//! Package-authored catalogs consumed by the production host.
//!
//! Identities are loaded from the locked registration image when it carries
//! them, otherwise from the shipped package JSON. This module does not embed
//! Terrenia block, tool, or recipe identifiers.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU8, NonZeroU32},
};

use latticeaxiom_content::{
    ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1, FluidDefinitionV1,
};
use latticeaxiom_core::StableId;
use latticeaxiom_gameplay::{
    BlockDefinitionV1, BlockId, CatalogLimits, FrozenItemRoleBindingV1, GameplayCatalog,
    GameplayCatalogSourceV1, IngredientV1, ItemDefinitionV1, ItemId, ItemPredicateV1,
    ItemRoleDefinitionV1, ItemRoleId, MiningRuleV1, RecipeDefinitionV1, RecipeId, RecipePatternV1,
    RoleOutputV1, ToolClassId, ToolDefinitionV1, ToolRequirementV1, WorkstationDefinitionV1,
    WorkstationId,
};
use latticeaxiom_storage::DimensionId;
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1,
    FrozenRoleBindingsV1,
};
use serde::Deserialize;
use serde_json::Value;

use super::ProductionHostError;
use crate::LockVerifiedComposeImages;

const AUTHORED_BLOCKS_JSON: &str =
    include_str!("../../../../packages/terrenia/blocks/data/authored-catalog-v1.json");
const AUTHORED_RULES_JSON: &str =
    include_str!("../../../../packages/terrenia/gameplay/data/authored-rules-v1.json");
const AUTHORED_TOOLS_JSON: &str =
    include_str!("../../../../packages/terrenia/tools/data/authored-tools-v1.json");
const AUTHORED_BINDINGS_JSON: &str =
    include_str!("../../../../packages/terrenia/worldgen/data/authored-block-bindings-v1.json");
const D9_BLOCK_IDS: &str =
    include_str!("../../../../packages/terrenia/blocks/data/goldens/d9-block-ids.txt");

/// Worldgen identities compiled from the package catalog.
#[derive(Clone, Debug)]
pub(super) struct HostWorldgenCatalog {
    pub(super) dimension: DimensionId,
    pub(super) palette: Vec<BlockId>,
    pub(super) empty: BlockId,
    pub(super) placement_content: BlockId,
    pub(super) probe_content: BlockId,
    pub(super) role_vocabulary: D4RoleVocabularyV1,
    pub(super) role_bindings: FrozenRoleBindingsV1,
    pub(super) block_catalog: D4BlockCatalogClosureV1,
    pub(super) bindings: AuthoredWorldgenBindingsV1,
}

/// Compiles the package-authored gameplay catalog.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when authored JSON is invalid or a required
/// item, block, tool, or recipe definition is missing.
pub fn authored_gameplay_catalog() -> Result<GameplayCatalog, ProductionHostError> {
    let catalog = GameplayCatalog::compile(authored_gameplay_source()?, CatalogLimits::default())
        .map_err(ProductionHostError::from)?;
    require_compiled_gameplay_covers_d9(&catalog)?;
    Ok(catalog)
}

/// Compiles an empty gameplay catalog for hosts that only edit voxels.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when the empty source fails compilation.
pub fn empty_gameplay_catalog() -> Result<GameplayCatalog, ProductionHostError> {
    Ok(GameplayCatalog::compile(
        GameplayCatalogSourceV1::default(),
        CatalogLimits::default(),
    )?)
}

/// Compiles the authored D9 block and fluid catalog used by occupancy.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when the package JSON is invalid or the
/// compiler rejects a definition.
pub fn authored_content_catalog() -> Result<ContentCatalogV1, ProductionHostError> {
    let authored = parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")?;
    let blocks = json_array(&authored, "blocks")?
        .iter()
        .map(|row| {
            decode_catalog_row::<latticeaxiom_content::BlockDefinitionV1>(row, "block-definition")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let fluids = json_array(&authored, "fluids")?
        .iter()
        .map(|row| decode_catalog_row::<FluidDefinitionV1>(row, "fluid-definition"))
        .collect::<Result<Vec<_>, _>>()?;
    let material_role_bindings = match authored.get("material_role_bindings") {
        Some(value) => serde_json::from_value(value.clone()).map_err(|source| {
            ProductionHostError::InvalidAuthoredCatalog {
                name: "material-role-bindings",
                source,
            }
        })?,
        None => Vec::new(),
    };
    let catalog = ContentCatalogV1::compile(
        ContentCatalogInputV1 {
            schema_major: 1,
            blocks,
            fluids,
            biomes: Vec::new(),
            material_role_bindings,
        },
        ContentCatalogLimitsV1::default(),
    )?;
    require_d9_golden_block_ids(
        &catalog
            .blocks()
            .iter()
            .map(|block| block.definition().header.stable_id.clone())
            .collect(),
    )?;
    Ok(catalog)
}

fn decode_catalog_row<T>(row: &Value, name: &'static str) -> Result<T, ProductionHostError>
where
    T: for<'de> Deserialize<'de>,
{
    let definition =
        row.get("definition")
            .cloned()
            .ok_or(ProductionHostError::InvalidCatalogField {
                field: "definition",
            })?;
    serde_json::from_value(definition)
        .map_err(|source| ProductionHostError::InvalidAuthoredCatalog { name, source })
}

pub(super) fn host_worldgen_catalog(
    images: &LockVerifiedComposeImages,
) -> Result<HostWorldgenCatalog, ProductionHostError> {
    let catalog_ids =
        authored_catalog_block_ids(&parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")?)?;
    require_d9_golden_block_ids(&catalog_ids)?;
    let bindings = AuthoredWorldgenBindingsV1::from_json(AUTHORED_BINDINGS_JSON.as_bytes())?;
    let authored: AuthoredBlockBindings =
        serde_json::from_str(AUTHORED_BINDINGS_JSON).map_err(|source| {
            ProductionHostError::InvalidAuthoredCatalog {
                name: "worldgen-block-bindings",
                source,
            }
        })?;
    if authored.roles.is_empty() {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "worldgen-role",
            id: "roles".to_owned(),
        });
    }

    let mut roles_by_path = BTreeMap::<String, (StableId, StableId)>::new();
    for row in &authored.roles {
        let role: StableId = row.id.parse()?;
        let candidate: StableId = row.candidate.parse()?;
        if role.kind() != "block-role" {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "block-role",
                id: row.id.clone(),
            });
        }
        if candidate.kind() != "block" || !catalog_ids.contains(&candidate) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "block",
                id: row.candidate.clone(),
            });
        }
        if roles_by_path
            .insert(role.path().to_owned(), (role, candidate))
            .is_some()
        {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "unique-block-role-path",
                id: row.id.clone(),
            });
        }
    }

    let mut vocabulary_entries = Vec::new();
    let mut binding_entries = Vec::new();
    for purpose in D4MaterialRoleV1::ALL {
        let Some((role, target)) = roles_by_path.get(purpose.as_str()) else {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "d4-role",
                id: purpose.as_str().to_owned(),
            });
        };
        vocabulary_entries.push((purpose, role.clone()));
        binding_entries.push((role.clone(), target.clone()));
    }

    let block_catalog = D4BlockCatalogClosureV1::new(catalog_ids)?;
    let palette = block_catalog
        .blocks()
        .iter()
        .map(|block| BlockId::parse(block.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    let role_vocabulary = D4RoleVocabularyV1::new(vocabulary_entries)?;
    let role_bindings = FrozenRoleBindingsV1::new(binding_entries)?;
    let empty = bound_block(&role_vocabulary, &role_bindings, D4MaterialRoleV1::Empty)?;
    let placement_content = bound_block(
        &role_vocabulary,
        &role_bindings,
        D4MaterialRoleV1::TemperateSubsurface,
    )?;
    let probe_content = bound_block(
        &role_vocabulary,
        &role_bindings,
        D4MaterialRoleV1::TemperateBaseRock,
    )?;
    for required in [&empty, &placement_content, &probe_content] {
        if !palette.iter().any(|block| block == required) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "palette-block",
                id: required.as_str().to_owned(),
            });
        }
    }

    Ok(HostWorldgenCatalog {
        dimension: resolve_dimension(images, &palette)?,
        palette,
        empty,
        placement_content,
        probe_content,
        role_vocabulary,
        role_bindings,
        block_catalog,
        bindings,
    })
}

fn bound_block(
    vocabulary: &D4RoleVocabularyV1,
    bindings: &FrozenRoleBindingsV1,
    purpose: D4MaterialRoleV1,
) -> Result<BlockId, ProductionHostError> {
    let role = vocabulary.role(purpose);
    let target =
        bindings
            .target(role)
            .ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
                kind: "role-binding",
                id: role.as_str().to_owned(),
            })?;
    Ok(BlockId::parse(target.as_str())?)
}

fn resolve_dimension(
    images: &LockVerifiedComposeImages,
    palette: &[BlockId],
) -> Result<DimensionId, ProductionHostError> {
    let mut registered = images
        .images()
        .registration()
        .image
        .numeric_ids
        .keys()
        .filter(|id| id.kind() == "dimension")
        .cloned()
        .collect::<Vec<_>>();
    registered.sort();
    match registered.as_slice() {
        [] => catalog_namespace_dimension(palette),
        [id] => Ok(id.as_str().parse()?),
        _ => Err(ProductionHostError::AmbiguousDimension),
    }
}

fn catalog_namespace_dimension(palette: &[BlockId]) -> Result<DimensionId, ProductionHostError> {
    let Some(first) = palette.first() else {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "dimension",
            id: "catalog".to_owned(),
        });
    };
    let namespace = first.namespace();
    format!("{namespace}:dimension/{namespace}")
        .parse()
        .map_err(ProductionHostError::from)
}

fn d9_golden_block_ids() -> Result<BTreeSet<StableId>, ProductionHostError> {
    let mut ids = BTreeSet::new();
    for line in D9_BLOCK_IDS.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let id: StableId = line.parse()?;
        if id.kind() != "block" {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "block",
                id: line.to_owned(),
            });
        }
        if !ids.insert(id) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "unique-d9-block",
                id: line.to_owned(),
            });
        }
    }
    if ids.is_empty() {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "d9-block",
            id: "goldens/d9-block-ids.txt".to_owned(),
        });
    }
    Ok(ids)
}

fn authored_catalog_block_ids(catalog: &Value) -> Result<BTreeSet<StableId>, ProductionHostError> {
    let mut ids = BTreeSet::new();
    for row in json_array(catalog, "blocks")? {
        let definition = row
            .get("definition")
            .ok_or(ProductionHostError::InvalidCatalogField {
                field: "definition",
            })?;
        let header = definition
            .get("header")
            .ok_or(ProductionHostError::InvalidCatalogField { field: "header" })?;
        let text = json_text(header, "stable_id")?;
        let id: StableId = text.parse()?;
        if id.kind() != "block" {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "block",
                id: text.to_owned(),
            });
        }
        if !ids.insert(id) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "unique-block",
                id: text.to_owned(),
            });
        }
    }
    Ok(ids)
}

fn require_d9_golden_block_ids(present: &BTreeSet<StableId>) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d9-block",
        d9_golden_block_ids()?
            .into_iter()
            .filter(|id| !present.contains(id))
            .map(|id| id.as_str().to_owned()),
    )
}

fn require_compiled_gameplay_covers_d9(
    catalog: &GameplayCatalog,
) -> Result<(), ProductionHostError> {
    let blocks = parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")?;
    let mut unmineable = BTreeSet::new();
    for row in json_array(&blocks, "blocks")? {
        if row
            .pointer("/physical/hardness_ticks")
            .and_then(Value::as_u64)
            != Some(0)
        {
            continue;
        }
        if let Some(id) = row
            .pointer("/definition/header/stable_id")
            .and_then(Value::as_str)
        {
            unmineable.insert(id.to_owned());
        }
    }
    let mut missing = Vec::new();
    for id in d9_golden_block_ids()? {
        let block = BlockId::parse(id.as_str())?;
        if catalog.block(&block).is_none() && !unmineable.contains(id.as_str()) {
            missing.push(id.as_str().to_owned());
        }
    }
    missing_catalog_ids("gameplay-block", missing)
}

fn missing_catalog_ids(
    kind: &'static str,
    missing: impl IntoIterator<Item = String>,
) -> Result<(), ProductionHostError> {
    let missing = missing.into_iter().collect::<Vec<_>>();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(ProductionHostError::MissingCatalogDefinition {
            kind,
            id: missing.join(","),
        })
    }
}

fn authored_gameplay_source() -> Result<GameplayCatalogSourceV1, ProductionHostError> {
    let blocks = parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")?;
    require_d9_golden_block_ids(&authored_catalog_block_ids(&blocks)?)?;
    let rules = parse_json_object(AUTHORED_RULES_JSON, "gameplay-rules")?;
    let tools = parse_json_object(AUTHORED_TOOLS_JSON, "tools")?;
    let mut items = BTreeMap::new();
    let mut item_roles = Vec::new();
    let mut bindings = Vec::new();
    let mut recipes = Vec::new();
    let mut workstations = BTreeMap::new();
    ingest_items(json_array(&rules, "items")?, &mut items)?;
    ingest_items(json_array(&tools, "items")?, &mut items)?;
    ingest_roles(
        json_array(&rules, "recipe_output_roles")?,
        &mut item_roles,
        &mut bindings,
    )?;
    ingest_roles(
        json_array(&tools, "recipe_output_roles")?,
        &mut item_roles,
        &mut bindings,
    )?;
    ingest_recipes(
        json_array(&rules, "recipes")?,
        &mut recipes,
        &mut workstations,
        false,
    )?;
    ingest_recipes(
        json_array(&tools, "recipes")?,
        &mut recipes,
        &mut workstations,
        true,
    )?;
    let drop_tables = drop_table_map(json_array(&rules, "drop_tables")?)?;
    let tool_requirements = tool_requirement_map(json_array(&rules, "tool_requirements")?)?;
    Ok(GameplayCatalogSourceV1 {
        items: items.into_values().collect(),
        blocks: compile_blocks(
            json_array(&blocks, "blocks")?,
            &drop_tables,
            &tool_requirements,
        )?,
        tools: compile_tools(json_array(&tools, "tools")?)?,
        roles: item_roles,
        bindings,
        recipes,
        workstations: workstations.into_values().collect(),
        ..GameplayCatalogSourceV1::default()
    })
}

fn parse_json_object(source: &str, name: &'static str) -> Result<Value, ProductionHostError> {
    let value: Value = serde_json::from_str(source).map_err(|error| {
        ProductionHostError::InvalidAuthoredCatalog {
            name,
            source: error,
        }
    })?;
    if value.as_object().is_none() {
        return Err(ProductionHostError::InvalidCatalogField { field: name });
    }
    Ok(value)
}

fn ingest_items(
    rows: &[Value],
    items: &mut BTreeMap<String, ItemDefinitionV1>,
) -> Result<(), ProductionHostError> {
    for row in rows {
        let id = json_text(row, "id")?;
        let item = ItemId::parse(id)?;
        let durability = json_optional_u32(row, "durability")?.and_then(NonZeroU32::new);
        let placement = match row.get("placement_block") {
            Some(Value::String(value)) => Some(BlockId::parse(value)?),
            Some(_) => {
                return Err(ProductionHostError::InvalidCatalogField {
                    field: "placement_block",
                });
            }
            None => None,
        };
        let stack = json_optional_u32(row, "maximum_stack")?
            .and_then(NonZeroU32::new)
            .unwrap_or(NonZeroU32::MIN);
        items.insert(
            id.to_owned(),
            ItemDefinitionV1 {
                id: item,
                stack_limit: if durability.is_some() {
                    NonZeroU32::MIN
                } else {
                    stack
                },
                placement_block: placement,
                durability,
            },
        );
    }
    Ok(())
}

fn ingest_roles(
    rows: &[Value],
    roles: &mut Vec<ItemRoleDefinitionV1>,
    bindings: &mut Vec<FrozenItemRoleBindingV1>,
) -> Result<(), ProductionHostError> {
    for row in rows {
        let id = json_text(row, "id")?;
        let item = json_text(row, "concrete_item")?;
        let role = ItemRoleId::parse(id)?;
        let concrete = ItemId::parse(item)?;
        roles.push(ItemRoleDefinitionV1 {
            id: role.clone(),
            accepts: ItemPredicateV1::Exact(concrete.clone()),
        });
        bindings.push(FrozenItemRoleBindingV1 {
            role,
            item: concrete,
        });
    }
    Ok(())
}

fn ingest_recipes(
    rows: &[Value],
    recipes: &mut Vec<RecipeDefinitionV1>,
    workstations: &mut BTreeMap<String, WorkstationDefinitionV1>,
    require_pattern: bool,
) -> Result<(), ProductionHostError> {
    for row in rows {
        let kind = json_text(row, "kind")?;
        if kind == "tool-interaction" {
            continue;
        }
        let id = json_text(row, "id")?;
        let Some(pattern) = recipe_pattern(row, kind, require_pattern)? else {
            continue;
        };
        let recipe = RecipeId::parse(id)?;
        let workstation = match row.get("workstation") {
            Some(Value::String(value)) => {
                let workstation = WorkstationId::parse(value)?;
                workstations.insert(
                    value.clone(),
                    WorkstationDefinitionV1 {
                        id: workstation.clone(),
                    },
                );
                Some(workstation)
            }
            Some(_) => {
                return Err(ProductionHostError::InvalidCatalogField {
                    field: "workstation",
                });
            }
            None => None,
        };
        recipes.push(RecipeDefinitionV1 {
            id: recipe,
            workstation,
            pattern,
            output: RoleOutputV1 {
                role: ItemRoleId::parse(json_text(row, "output_role")?)?,
                quantity: json_quantity(&row["output"], "quantity")?,
            },
        });
    }
    Ok(())
}

fn recipe_pattern(
    row: &Value,
    kind: &str,
    require_pattern: bool,
) -> Result<Option<RecipePatternV1>, ProductionHostError> {
    if kind == "shapeless" {
        let ingredients = json_array(row, "inputs")?
            .iter()
            .map(|input| {
                Ok(IngredientV1 {
                    accepts: ItemPredicateV1::Exact(ItemId::parse(json_text(input, "item")?)?),
                    quantity: json_quantity(input, "quantity")?,
                })
            })
            .collect::<Result<Vec<_>, ProductionHostError>>()?;
        return Ok(Some(RecipePatternV1::Shapeless {
            ingredients: ingredients.into_boxed_slice(),
        }));
    }
    if kind != "shaped" {
        return Ok(None);
    }
    if let Some(pattern) = row.get("pattern").and_then(Value::as_array) {
        let width = u8::try_from(json_u64(row, "width")?)
            .ok()
            .and_then(NonZeroU8::new)
            .ok_or(ProductionHostError::InvalidCatalogField { field: "width" })?;
        let height = u8::try_from(json_u64(row, "height")?)
            .ok()
            .and_then(NonZeroU8::new)
            .ok_or(ProductionHostError::InvalidCatalogField { field: "height" })?;
        let mut cells = Vec::new();
        for line in pattern {
            let Some(line) = line.as_array() else {
                return Err(ProductionHostError::InvalidCatalogField { field: "pattern" });
            };
            for cell in line {
                cells.push(match cell {
                    Value::Null => None,
                    Value::String(item) => Some(IngredientV1 {
                        accepts: ItemPredicateV1::Exact(ItemId::parse(item)?),
                        quantity: NonZeroU32::MIN,
                    }),
                    _ => {
                        return Err(ProductionHostError::InvalidCatalogField { field: "pattern" });
                    }
                });
            }
        }
        return Ok(Some(RecipePatternV1::Shaped {
            width,
            height,
            cells: cells.into_boxed_slice(),
        }));
    }
    if require_pattern {
        return Ok(None);
    }
    let inputs = json_array(row, "inputs")?;
    if inputs.len() == 1 && json_quantity(&inputs[0], "quantity")?.get() == 4 {
        let plank = ItemId::parse(json_text(&inputs[0], "item")?)?;
        let cell = Some(IngredientV1 {
            accepts: ItemPredicateV1::Exact(plank),
            quantity: NonZeroU32::MIN,
        });
        let two = NonZeroU8::new(2).ok_or(ProductionHostError::InvalidCatalogField {
            field: "shaped-grid",
        })?;
        return Ok(Some(RecipePatternV1::Shaped {
            width: two,
            height: two,
            cells: vec![cell.clone(), cell.clone(), cell.clone(), cell].into_boxed_slice(),
        }));
    }
    Ok(None)
}

fn compile_blocks(
    rows: &[Value],
    drop_tables: &BTreeMap<String, latticeaxiom_gameplay::ItemStackV1>,
    tool_requirements: &BTreeMap<String, Option<ToolRequirementV1>>,
) -> Result<Vec<BlockDefinitionV1>, ProductionHostError> {
    let mut blocks = Vec::new();
    for row in rows {
        let definition = row
            .get("definition")
            .ok_or(ProductionHostError::InvalidCatalogField {
                field: "definition",
            })?;
        let header = definition
            .get("header")
            .ok_or(ProductionHostError::InvalidCatalogField { field: "header" })?;
        let id = json_text(header, "stable_id")?;
        let ticks = row
            .pointer("/physical/hardness_ticks")
            .and_then(Value::as_u64)
            .ok_or(ProductionHostError::InvalidCatalogField {
                field: "hardness_ticks",
            })?;
        if ticks == 0 {
            continue;
        }
        let hardness = u32::try_from(ticks).ok().and_then(NonZeroU32::new).ok_or(
            ProductionHostError::InvalidCatalogField {
                field: "hardness_ticks",
            },
        )?;
        let rules = definition
            .get("rules")
            .ok_or(ProductionHostError::InvalidCatalogField { field: "rules" })?;
        let drop_id = json_text(rules, "drop_table")?;
        let Some(drop) = drop_tables.get(drop_id).cloned() else {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "drop-table",
                id: drop_id.to_owned(),
            });
        };
        let tool_id = json_text(row, "tool_requirement")?;
        let tool = tool_requirements.get(tool_id).cloned().ok_or_else(|| {
            ProductionHostError::MissingCatalogDefinition {
                kind: "tool-requirement",
                id: tool_id.to_owned(),
            }
        })?;
        blocks.push(BlockDefinitionV1 {
            id: BlockId::parse(id)?,
            mining: MiningRuleV1 { hardness, tool },
            drop,
        });
    }
    Ok(blocks)
}

fn compile_tools(rows: &[Value]) -> Result<Vec<ToolDefinitionV1>, ProductionHostError> {
    rows.iter()
        .map(|row| {
            let item = json_text(row, "item")?;
            let class = json_text(row, "class")?;
            let durability = json_quantity(row, "durability")?;
            let multiplier = json_optional_u32(row, "mining_multiplier")?
                .and_then(NonZeroU32::new)
                .unwrap_or(NonZeroU32::MIN);
            let tier = u8::try_from(json_u64(row, "tier")?)
                .map_err(|_| ProductionHostError::InvalidCatalogField { field: "tier" })?;
            Ok(ToolDefinitionV1 {
                item: ItemId::parse(item)?,
                class: ToolClassId::parse(class)?,
                tier,
                work_per_step: multiplier,
                maximum_durability: durability,
            })
        })
        .collect()
}

fn drop_table_map(
    rows: &[Value],
) -> Result<BTreeMap<String, latticeaxiom_gameplay::ItemStackV1>, ProductionHostError> {
    let mut tables = BTreeMap::new();
    for row in rows {
        let outputs = json_array(row, "outputs")?;
        if outputs.len() != 1 {
            continue;
        }
        let item = json_text(&outputs[0], "item")?;
        let quantity = json_quantity(&outputs[0], "quantity")?;
        tables.insert(
            json_text(row, "id")?.to_owned(),
            latticeaxiom_gameplay::ItemStackV1::plain(ItemId::parse(item)?, quantity.get())?,
        );
    }
    Ok(tables)
}

fn tool_requirement_map(
    rows: &[Value],
) -> Result<BTreeMap<String, Option<ToolRequirementV1>>, ProductionHostError> {
    let mut requirements = BTreeMap::new();
    for row in rows {
        let class = json_text(row, "tool_class")?;
        let minimum_tier = u8::try_from(json_u64(row, "minimum_tier")?).map_err(|_| {
            ProductionHostError::InvalidCatalogField {
                field: "minimum_tier",
            }
        })?;
        let required =
            if matches!(class, "none" | "hand") || (class != "pickaxe" && minimum_tier == 0) {
                None
            } else {
                let id = if class.contains(':') {
                    class.to_owned()
                } else {
                    format!("latticeaxiom:tool-class/{class}@1")
                };
                Some(ToolRequirementV1 {
                    class: ToolClassId::parse(&id)?,
                    minimum_tier,
                })
            };
        requirements.insert(json_text(row, "id")?.to_owned(), required);
    }
    Ok(requirements)
}

fn json_array<'a>(value: &'a Value, key: &'static str) -> Result<&'a [Value], ProductionHostError> {
    match value.get(key) {
        Some(Value::Array(rows)) => Ok(rows),
        Some(_) => Err(ProductionHostError::InvalidCatalogField { field: key }),
        None => Ok(&[]),
    }
}

fn json_text<'a>(value: &'a Value, key: &'static str) -> Result<&'a str, ProductionHostError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ProductionHostError::InvalidCatalogField { field: key })
}

fn json_u64(value: &Value, key: &'static str) -> Result<u64, ProductionHostError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .ok_or(ProductionHostError::InvalidCatalogField { field: key })
}

fn json_optional_u32(value: &Value, key: &'static str) -> Result<Option<u32>, ProductionHostError> {
    match value.get(key) {
        None => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .map(Some)
            .ok_or(ProductionHostError::InvalidCatalogField { field: key }),
    }
}

fn json_quantity(value: &Value, key: &'static str) -> Result<NonZeroU32, ProductionHostError> {
    let quantity = json_optional_u32(value, key)?.unwrap_or(1);
    NonZeroU32::new(quantity).ok_or(ProductionHostError::InvalidCatalogField { field: key })
}

#[derive(Debug, Deserialize)]
struct AuthoredBlockBindings {
    roles: Vec<AuthoredRoleRow>,
}

#[derive(Debug, Deserialize)]
struct AuthoredRoleRow {
    id: String,
    candidate: String,
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::BTreeSet;

    use super::{
        AUTHORED_BLOCKS_JSON, authored_catalog_block_ids, authored_content_catalog,
        authored_gameplay_catalog, d9_golden_block_ids, parse_json_object,
    };
    use latticeaxiom_gameplay::BlockId;

    #[test]
    fn d9_golden_block_ids_are_present_in_authored_catalog() {
        let authored = authored_catalog_block_ids(
            &parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")
                .expect("Terrenia authored catalog is valid JSON"),
        )
        .expect("authored catalog block IDs are valid");
        let golden = d9_golden_block_ids().expect("D9 golden IDs are valid");
        assert_eq!(golden.len(), 72);
        let missing = golden
            .iter()
            .filter(|id| !authored.contains(*id))
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "authored catalog is missing golden IDs: {missing:?}"
        );
    }

    #[test]
    fn compiled_gameplay_catalog_contains_every_d9_golden_id() {
        let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
        let golden = d9_golden_block_ids().expect("D9 golden IDs are valid");
        let missing = golden
            .iter()
            .filter_map(|id| {
                let block = BlockId::parse(id.as_str())
                    .expect("golden block IDs satisfy the gameplay-ID contract");
                catalog
                    .block(&block)
                    .is_none()
                    .then(|| id.as_str().to_owned())
            })
            .collect::<Vec<_>>();
        assert_eq!(
            missing,
            vec!["terrenia:block/air".to_owned()],
            "compiled GameplayCatalog is missing golden IDs: {missing:?}"
        );
        assert_eq!(catalog.blocks().len(), golden.len() - missing.len());
    }

    #[test]
    fn compiled_content_catalog_contains_every_d9_golden_id() {
        let catalog = authored_content_catalog().expect("package content catalog must compile");
        let golden = d9_golden_block_ids().expect("D9 golden IDs are valid");
        let compiled = catalog
            .blocks()
            .iter()
            .map(|block| block.definition().header.stable_id.clone())
            .collect::<BTreeSet<_>>();
        let missing = golden
            .iter()
            .filter(|id| !compiled.contains(*id))
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "compiled content catalog is missing golden IDs: {missing:?}"
        );
        assert_eq!(golden.len(), 72);
        assert_eq!(compiled, golden);
    }
}
