//! Package-authored catalogs consumed by the production host.
//!
//! Identities are loaded from the locked registration image when it carries
//! them, otherwise from the shipped package JSON. This module does not embed
//! Terrenia block, tool, or recipe identifiers.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU8, NonZeroU16, NonZeroU32},
};

use latticeaxiom_compose::LockedPackage;
use latticeaxiom_content::{
    BiomeDefinitionV1, ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1,
    FluidDefinitionV1,
};
use latticeaxiom_core::{CapabilityId, PackageName, SchemaId, StableId};
use latticeaxiom_gameplay::{
    BlockDefinitionV1, BlockId, BlockSchemaBindingV1, CatalogLimits, FrozenItemRoleBindingV1,
    GameplayCatalog, GameplayCatalogSourceV1, IngredientV1, ItemDefinitionV1, ItemId,
    ItemPredicateV1, ItemRoleDefinitionV1, ItemRoleId, MiningRuleV1, RecipeDefinitionV1, RecipeId,
    RecipePatternV1, RoleOutputV1, ToolClassId, ToolDefinitionV1, ToolRequirementV1,
    WorkstationDefinitionV1, WorkstationId,
};
use latticeaxiom_storage::DimensionId;
use latticeaxiom_worldgen::{
    AuthoredWorldgenBindingsV1, CaveTopologyAlgorithmV1, D4BlockCatalogClosureV1, D4MaterialRoleV1,
    D4RoleVocabularyV1, D7_NATURAL_BLOCK_COUNT, FrozenRoleBindingsV1, HydrologyFluidBindingsV1,
    HydrologyOccupancyConfigV1, HydrologyOccupancyInputV1, NaturalRoleVocabularyV1,
    WorldgenConfigV1,
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
const AUTHORED_BIOMES_JSON: &str =
    include_str!("../../../../packages/terrenia/worldgen/data/authored-biomes-v1.json");
const AUTHORED_NATURAL_LAYERS_JSON: &str =
    include_str!("../../../../packages/terrenia/worldgen/data/authored-natural-layers-v1.json");
const D7_BLOCK_IDS: &str =
    include_str!("../../../../packages/terrenia/blocks/data/goldens/d7-block-ids.txt");
const D7_BIOME_IDS: &str =
    include_str!("../../../../packages/terrenia/worldgen/data/goldens/d7-biome-ids.txt");
const D7_NATURAL_ROLE_IDS: &str =
    include_str!("../../../../packages/terrenia/worldgen/data/goldens/d7-natural-role-ids.txt");
const D9_BLOCK_IDS: &str =
    include_str!("../../../../packages/terrenia/blocks/data/goldens/d9-block-ids.txt");
/// Exactly-one terrain/worldgen provider selected by a reopened product lock.
pub(super) const WORLDGEN_TERRAIN_CAPABILITY: &str =
    "latticeaxiom:capability/worldgen-terrain-provider@2";
/// Exactly-one content-blocks provider selected by a reopened product lock.
pub(super) const CONTENT_BLOCKS_CAPABILITY: &str = "latticeaxiom:capability/content-blocks@1";

/// Worldgen identities compiled from the package catalog.
#[derive(Clone, Debug)]
pub(super) struct HostWorldgenCatalog {
    pub(super) dimension: DimensionId,
    pub(super) palette: Vec<BlockId>,
    pub(super) empty: BlockId,
    pub(super) placement_content: BlockId,
    pub(super) probe_content: BlockId,
    pub(super) role_vocabulary: D4RoleVocabularyV1,
    pub(super) natural_vocabulary: NaturalRoleVocabularyV1,
    pub(super) role_bindings: FrozenRoleBindingsV1,
    pub(super) block_catalog: D4BlockCatalogClosureV1,
    pub(super) bindings: AuthoredWorldgenBindingsV1,
    pub(super) worldgen_package: Option<LockedPackage>,
    pub(super) cave: HostCaveBindings,
    pub(super) hydrology: HostHydrologyBindings,
}

/// Package-owned cave topology identities bound from authored worldgen data.
#[derive(Clone, Debug)]
pub(super) struct HostCaveBindings {
    pub(super) default_domain: StableId,
    pub(super) domains: Vec<HostCaveDomain>,
}

/// One underground-owned topology domain and its local algorithm.
#[derive(Clone, Debug)]
pub(super) struct HostCaveDomain {
    pub(super) id: StableId,
    pub(super) algorithm: CaveTopologyAlgorithmV1,
}

/// Frozen water/lava occupancy identities bound from the locked catalog.
#[derive(Clone, Debug)]
pub(super) struct HostHydrologyBindings {
    pub(super) fluids: HydrologyFluidBindingsV1,
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
    let biomes = authored_biome_definitions()?;
    require_d7_golden_biome_ids(
        &biomes
            .iter()
            .map(|biome| biome.header.stable_id.clone())
            .collect(),
    )?;
    let catalog = ContentCatalogV1::compile(
        ContentCatalogInputV1 {
            schema_major: 1,
            blocks,
            fluids,
            biomes,
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

#[allow(clippy::too_many_lines)]
pub(super) fn host_worldgen_catalog(
    images: &LockVerifiedComposeImages,
) -> Result<HostWorldgenCatalog, ProductionHostError> {
    let worldgen_package = exactly_one_lock_provider(images, WORLDGEN_TERRAIN_CAPABILITY)?;
    let _content_package = exactly_one_lock_provider(images, CONTENT_BLOCKS_CAPABILITY)?;
    let catalog_ids =
        authored_catalog_block_ids(&parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")?)?;
    require_d7_golden_block_ids(&catalog_ids)?;
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
    for purpose in D4MaterialRoleV1::ALL {
        let Some((role, _)) = roles_by_path.get(purpose.as_str()) else {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "d4-role",
                id: purpose.as_str().to_owned(),
            });
        };
        vocabulary_entries.push((purpose, role.clone()));
    }

    let block_catalog = D4BlockCatalogClosureV1::new(catalog_ids)?;
    if block_catalog.blocks().len() < D7_NATURAL_BLOCK_COUNT {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "d7-natural-block",
            id: D7_NATURAL_BLOCK_COUNT.to_string(),
        });
    }
    let palette = block_catalog
        .blocks()
        .iter()
        .map(|block| BlockId::parse(block.as_str()))
        .collect::<Result<Vec<_>, _>>()?;
    let role_vocabulary = D4RoleVocabularyV1::new(vocabulary_entries)?;
    let natural_vocabulary = bindings.natural_vocabulary()?;
    let role_bindings = bindings.role_bindings()?;
    require_d7_natural_role_ids(&role_bindings)?;
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
        natural_vocabulary,
        role_bindings,
        block_catalog,
        cave: authored_cave_bindings()?,
        hydrology: authored_hydrology_bindings(&bindings)?,
        bindings,
        worldgen_package,
    })
}

impl HostWorldgenCatalog {
    /// Compiles the package-owned V6 cave topology layer for `config`.
    ///
    /// Domain identities come from the authored worldgen package. Geometry is
    /// derived from the closed spine config so spawn-neighborhood traversal
    /// reaches both underground territories.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when domain identities, algorithms, or
    /// topology contracts fail closed.
    pub(super) fn cave_topology_layer(
        &self,
        config: &WorldgenConfigV1,
    ) -> Result<latticeaxiom_worldgen::CaveTopologyLayerInputV1, ProductionHostError> {
        compile_host_cave_topology(&self.cave, config)
    }

    /// Compiles V6 hydrology occupancy from frozen catalog fluids and predicates.
    ///
    /// # Errors
    ///
    /// Returns [`ProductionHostError`] when occupancy configuration is invalid.
    pub(super) fn hydrology_occupancy(
        &self,
    ) -> Result<HydrologyOccupancyInputV1, ProductionHostError> {
        let config = HydrologyOccupancyConfigV1::default();
        config.validate()?;
        Ok(HydrologyOccupancyInputV1::new(
            config,
            self.hydrology.fluids.clone(),
        ))
    }
}

fn authored_cave_bindings() -> Result<HostCaveBindings, ProductionHostError> {
    let authored = parse_json_object(AUTHORED_NATURAL_LAYERS_JSON, "natural-layers")?;
    let cave_plan = authored
        .get("cave_plan")
        .ok_or(ProductionHostError::InvalidCatalogField { field: "cave_plan" })?;
    let default_domain = parse_identity(
        json_text(cave_plan, "default_topology_domain")?,
        "cave-topology-domain",
    )?;
    let mut domains = Vec::new();
    for row in json_array(&authored, "underground_territories")? {
        let id = parse_identity(json_text(row, "id")?, "cave-topology-domain")?;
        let algorithm = topology_algorithm_from_id(&parse_identity(
            json_text(row, "topology_algorithm")?,
            "cave-algorithm",
        )?)?;
        if domains
            .iter()
            .any(|domain: &HostCaveDomain| domain.id == id)
        {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "unique-cave-topology-domain",
                id: id.to_string(),
            });
        }
        domains.push(HostCaveDomain { id, algorithm });
    }
    domains.sort_by(|left, right| left.id.cmp(&right.id));
    if domains.len() < 2 {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "cave-topology-domain",
            id: "underground_territories".to_owned(),
        });
    }
    Ok(HostCaveBindings {
        default_domain,
        domains,
    })
}

fn authored_hydrology_bindings(
    bindings: &AuthoredWorldgenBindingsV1,
) -> Result<HostHydrologyBindings, ProductionHostError> {
    let authored = parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")?;
    let mut water = None;
    let mut lava = None;
    for row in json_array(&authored, "fluids")? {
        let definition = row
            .get("definition")
            .ok_or(ProductionHostError::InvalidCatalogField {
                field: "definition",
            })?;
        let header = definition
            .get("header")
            .ok_or(ProductionHostError::InvalidCatalogField { field: "header" })?;
        let id: StableId = json_text(header, "stable_id")?.parse()?;
        if id.kind() != "fluid" {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "fluid",
                id: id.to_string(),
            });
        }
        match last_path_token(&id) {
            "water" if water.is_none() => water = Some(id),
            "lava" if lava.is_none() => lava = Some(id),
            token => {
                return Err(ProductionHostError::MissingCatalogDefinition {
                    kind: "fluid-kind",
                    id: token.to_owned(),
                });
            }
        }
    }
    let water = water.ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
        kind: "fluid",
        id: "water".to_owned(),
    })?;
    let lava = lava.ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
        kind: "fluid",
        id: "lava".to_owned(),
    })?;
    let water_predicate = bindings.predicate("place-water").cloned().ok_or_else(|| {
        ProductionHostError::MissingCatalogDefinition {
            kind: "predicate",
            id: "place-water".to_owned(),
        }
    })?;
    let lava_predicate = bindings.predicate("place-lava").cloned().ok_or_else(|| {
        ProductionHostError::MissingCatalogDefinition {
            kind: "predicate",
            id: "place-lava".to_owned(),
        }
    })?;
    Ok(HostHydrologyBindings {
        fluids: HydrologyFluidBindingsV1::new(water, lava, water_predicate, lava_predicate)?,
    })
}

fn compile_host_cave_topology(
    cave: &HostCaveBindings,
    config: &WorldgenConfigV1,
) -> Result<latticeaxiom_worldgen::CaveTopologyLayerInputV1, ProductionHostError> {
    use latticeaxiom_worldgen::{
        CaveBranchContributorV1, CaveLayerCorridorV1, CaveLayerEntranceV1, CaveLayerPortalV1,
        CaveOwnedDomainV1, CaveTopologyLayerInputV1, cell_center_voxels,
    };

    if cave.domains.len() < 2 {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "cave-topology-domain",
            id: "underground".to_owned(),
        });
    }
    let first = &cave.domains[0];
    let second = &cave.domains[1];
    let floor = config.world_floor_y;
    let surface = config.temperate_base_height.max(floor.saturating_add(8));
    let cover = i32::from(config.cave_minimum_cover).max(1);
    let max_y = surface.saturating_sub(cover).max(floor.saturating_add(1));
    let cave_y = i64::from(max_y);
    let y_mm = cave_y.saturating_mul(1_000);
    let domains = vec![
        CaveOwnedDomainV1::new(
            first.id.clone(),
            first.algorithm,
            [0, 0],
            [1, 2],
            floor,
            surface,
        )?,
        CaveOwnedDomainV1::new(
            second.id.clone(),
            second.algorithm,
            [-1, 0],
            [0, 2],
            floor,
            surface,
        )?,
    ];
    let first_path = [
        cell_center_voxels(-2, 0, y_mm, config),
        cell_center_voxels(-1, 0, y_mm, config),
        cell_center_voxels(0, 0, y_mm, config),
        cell_center_voxels(1, 0, y_mm, config),
    ];
    let second_path = [
        cell_center_voxels(2, 0, y_mm, config),
        cell_center_voxels(1, 0, y_mm, config),
        cell_center_voxels(0, 0, y_mm, config),
        cell_center_voxels(-1, 0, y_mm, config),
    ];
    let corridors = vec![
        CaveLayerCorridorV1::new(first_path[0], first_path[1]),
        CaveLayerCorridorV1::new(first_path[1], first_path[2]),
        CaveLayerCorridorV1::new(first_path[2], first_path[3]),
        CaveLayerCorridorV1::new(second_path[0], second_path[1]),
        CaveLayerCorridorV1::new(second_path[1], second_path[2]),
        CaveLayerCorridorV1::new(second_path[2], second_path[3]),
    ];
    let portals = vec![
        CaveLayerPortalV1::new(first_path[1], 2, 3)?,
        CaveLayerPortalV1::new(first_path[2], 2, 3)?,
    ];
    let entrances = vec![
        CaveLayerEntranceV1::new(vec![[-2, 0], [-1, 0], [0, 0], [1, 0]], cave_y, [1, 0])?,
        CaveLayerEntranceV1::new(vec![[2, 0], [1, 0], [0, 0], [-1, 0]], cave_y, [-1, 0])?,
    ];
    let branch = CaveBranchContributorV1::new(first.id.clone(), [0, 1], [1, 2], floor, surface)?;
    Ok(CaveTopologyLayerInputV1::new(
        cave.default_domain.clone(),
        CaveTopologyAlgorithmV1::CoarseCell,
        domains,
        corridors,
        portals,
        entrances,
        branch,
    )?)
}

fn topology_algorithm_from_id(
    id: &StableId,
) -> Result<CaveTopologyAlgorithmV1, ProductionHostError> {
    match last_path_token(id) {
        "karst-graph" | "constrained-graph" => Ok(CaveTopologyAlgorithmV1::ConstrainedGraph),
        "chamber-graph" | "field-growth" => Ok(CaveTopologyAlgorithmV1::FieldGrowth),
        "coarse-cell" => Ok(CaveTopologyAlgorithmV1::CoarseCell),
        token => Err(ProductionHostError::MissingCatalogDefinition {
            kind: "cave-algorithm",
            id: token.to_owned(),
        }),
    }
}

fn parse_identity(value: &str, kind: &'static str) -> Result<StableId, ProductionHostError> {
    let id: StableId = value.parse()?;
    if id.kind() != kind {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind,
            id: value.to_owned(),
        });
    }
    Ok(id)
}

fn last_path_token(id: &StableId) -> &str {
    id.path()
        .rsplit('/')
        .next()
        .filter(|token| !token.is_empty())
        .unwrap_or(id.path())
}

fn authored_biome_definitions() -> Result<Vec<BiomeDefinitionV1>, ProductionHostError> {
    let authored = parse_json_object(AUTHORED_BIOMES_JSON, "biome-catalog")?;
    json_array(&authored, "biomes")?
        .iter()
        .map(|row| {
            serde_json::from_value(row.clone()).map_err(|source| {
                ProductionHostError::InvalidAuthoredCatalog {
                    name: "biome-definition",
                    source,
                }
            })
        })
        .collect()
}

/// Returns the exactly-one lock provider for `capability`, if the graph names it.
///
/// Synthetic headless fixtures may omit the capability. A declared empty or
/// duplicate provider list fails closed.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when the capability identity is invalid or
/// the lock lists zero or more than one provider.
pub(super) fn exactly_one_lock_provider(
    images: &LockVerifiedComposeImages,
    capability: &str,
) -> Result<Option<LockedPackage>, ProductionHostError> {
    let capability_id = capability.parse::<CapabilityId>()?;
    let Some(packages) = images
        .images()
        .graph()
        .capability_providers
        .get(&capability_id)
    else {
        return Ok(None);
    };
    match packages.as_slice() {
        [] => Err(ProductionHostError::MissingLockProvider {
            capability: capability.to_owned(),
        }),
        [name] => images
            .images()
            .graph()
            .packages
            .get(name)
            .cloned()
            .ok_or_else(|| ProductionHostError::MissingLockProvider {
                capability: capability.to_owned(),
            })
            .map(Some),
        _ => Err(ProductionHostError::AmbiguousLockProvider {
            capability: capability.to_owned(),
        }),
    }
}

/// Returns the registration namespace used by a scoped package name.
#[must_use]
pub(super) fn package_registration_namespace(package: &PackageName) -> &str {
    package
        .as_str()
        .strip_prefix('@')
        .and_then(|rest| rest.split('/').next())
        .filter(|namespace| !namespace.is_empty())
        .unwrap_or(package.as_str())
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

fn d7_golden_block_ids() -> Result<BTreeSet<StableId>, ProductionHostError> {
    golden_stable_ids(D7_BLOCK_IDS, "block", "d7-block")
}

fn d9_golden_block_ids() -> Result<BTreeSet<StableId>, ProductionHostError> {
    golden_stable_ids(D9_BLOCK_IDS, "block", "d9-block")
}

fn golden_stable_ids(
    source: &str,
    kind: &'static str,
    label: &'static str,
) -> Result<BTreeSet<StableId>, ProductionHostError> {
    let mut ids = BTreeSet::new();
    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let id: StableId = line.parse()?;
        if id.kind() != kind {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind,
                id: line.to_owned(),
            });
        }
        if !ids.insert(id) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: label,
                id: line.to_owned(),
            });
        }
    }
    if ids.is_empty() {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: label,
            id: "goldens".to_owned(),
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

fn require_d7_golden_biome_ids(present: &BTreeSet<StableId>) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d7-biome",
        golden_stable_ids(D7_BIOME_IDS, "biome", "d7-biome")?
            .into_iter()
            .filter(|id| !present.contains(id))
            .map(|id| id.as_str().to_owned()),
    )
}

fn require_d7_golden_block_ids(present: &BTreeSet<StableId>) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d7-block",
        d7_golden_block_ids()?
            .into_iter()
            .filter(|id| !present.contains(id))
            .map(|id| id.as_str().to_owned()),
    )
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

fn require_d7_natural_role_ids(bindings: &FrozenRoleBindingsV1) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d7-natural-role",
        golden_stable_ids(D7_NATURAL_ROLE_IDS, "block-role", "d7-natural-role")?
            .into_iter()
            .filter(|id| bindings.target(id).is_none())
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
    let block_schema_bindings = ingest_block_schema_bindings(
        json_array(&rules, "block_schema_bindings")?,
        &mut workstations,
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
        block_schema_bindings,
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

fn ingest_block_schema_bindings(
    rows: &[Value],
    workstations: &mut BTreeMap<String, WorkstationDefinitionV1>,
) -> Result<Vec<BlockSchemaBindingV1>, ProductionHostError> {
    let mut bindings = Vec::new();
    for row in rows {
        let block = BlockId::parse(json_text(row, "block")?)?;
        let mut schemas = Vec::new();
        for schema in json_array(row, "schemas")? {
            let text = schema
                .as_str()
                .ok_or(ProductionHostError::InvalidCatalogField { field: "schemas" })?;
            schemas.push(text.parse::<SchemaId>()?);
        }
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
        let container_slots = match row.get("container_slots") {
            Some(value) => {
                let slots = value.as_u64().and_then(|slots| u16::try_from(slots).ok());
                Some(slots.and_then(NonZeroU16::new).ok_or(
                    ProductionHostError::InvalidCatalogField {
                        field: "container_slots",
                    },
                )?)
            }
            None => None,
        };
        bindings.push(BlockSchemaBindingV1 {
            block,
            schemas: schemas.into_boxed_slice(),
            workstation,
            container_slots,
        });
    }
    Ok(bindings)
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
        authored_gameplay_catalog, d7_golden_block_ids, d9_golden_block_ids, parse_json_object,
    };
    use latticeaxiom_gameplay::{
        BlockId, ContainerOwnerComponentV1, ContainerStateV1, FurnaceContinuationV1,
        GameplayMutationIntentV1, ItemStackV1, SchemaId, WorkstationId,
    };
    use latticeaxiom_worldgen::D7_NATURAL_BLOCK_COUNT;

    #[test]
    fn d7_golden_block_ids_are_present_in_authored_catalog() {
        let authored = authored_catalog_block_ids(
            &parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")
                .expect("Terrenia authored catalog is valid JSON"),
        )
        .expect("authored catalog block IDs are valid");
        let golden = d7_golden_block_ids().expect("D7 golden IDs are valid");
        assert_eq!(golden.len(), D7_NATURAL_BLOCK_COUNT);
        let missing = golden
            .iter()
            .filter(|id| !authored.contains(*id))
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "authored catalog is missing D7 golden IDs: {missing:?}"
        );
    }

    #[test]
    fn compiled_content_catalog_includes_d7_biome_goldens() {
        let catalog = authored_content_catalog().expect("package content catalog must compile");
        let compiled = catalog
            .biomes()
            .iter()
            .map(|biome| biome.header.stable_id.clone())
            .collect::<BTreeSet<_>>();
        let golden = super::golden_stable_ids(super::D7_BIOME_IDS, "biome", "d7-biome")
            .expect("D7 biome goldens are valid");
        let missing = golden
            .iter()
            .filter(|id| !compiled.contains(*id))
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>();
        assert!(
            missing.is_empty(),
            "compiled content catalog is missing D7 biomes: {missing:?}"
        );
        assert_eq!(compiled, golden);
    }

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
    fn d9_workbench_furnace_chest_and_torch_bind_reserved_gameplay_schemas() {
        let catalog = authored_gameplay_catalog().expect("package gameplay catalog must compile");
        let parse_schema = |id: &str| {
            id.parse::<SchemaId>()
                .unwrap_or_else(|error| panic!("{id} is a reserved schema: {error}"))
        };
        let item_stack = parse_schema(ItemStackV1::SCHEMA_ID);
        let staged_edit = parse_schema(GameplayMutationIntentV1::SCHEMA_ID);
        let container = parse_schema(ContainerStateV1::SCHEMA_ID);
        let owner = parse_schema(ContainerOwnerComponentV1::SCHEMA_ID);
        let furnace_continuation = parse_schema(FurnaceContinuationV1::SCHEMA_ID);
        let crafting = WorkstationId::parse("latticeaxiom:workstation/crafting@1")
            .expect("crafting workstation is a platform contract");
        let furnace = WorkstationId::parse("latticeaxiom:workstation/furnace@1")
            .expect("furnace workstation is a platform contract");

        let torch = BlockId::parse("terrenia:block/torch").expect("D9 torch is canonical");
        let workbench =
            BlockId::parse("terrenia:block/workbench").expect("D9 workbench is canonical");
        let furnace_block =
            BlockId::parse("terrenia:block/furnace").expect("D9 furnace is canonical");
        let chest = BlockId::parse("terrenia:block/chest").expect("D9 chest is canonical");

        let torch_binding = catalog
            .block_schema_binding(&torch)
            .expect("torch binding is compiled");
        assert!(torch_binding.realizes(&item_stack));
        assert!(torch_binding.realizes(&staged_edit));
        assert!(!torch_binding.realizes_container());
        assert!(torch_binding.workstation.is_none());

        let workbench_binding = catalog
            .block_schema_binding(&workbench)
            .expect("workbench binding is compiled");
        assert!(workbench_binding.realizes(&container));
        assert!(workbench_binding.realizes(&owner));
        assert_eq!(workbench_binding.workstation.as_ref(), Some(&crafting));
        assert_eq!(workbench_binding.container_slot_count(), Some(9));
        assert_eq!(catalog.workstation_container_slots(&crafting), Some(9));

        let furnace_binding = catalog
            .block_schema_binding(&furnace_block)
            .expect("furnace binding is compiled");
        assert!(furnace_binding.realizes(&container));
        assert!(furnace_binding.realizes(&furnace_continuation));
        assert_eq!(furnace_binding.workstation.as_ref(), Some(&furnace));
        assert_eq!(furnace_binding.container_slot_count(), Some(3));

        let chest_binding = catalog
            .block_schema_binding(&chest)
            .expect("chest binding is compiled");
        assert!(chest_binding.realizes(&container));
        assert!(chest_binding.realizes(&owner));
        assert!(chest_binding.workstation.is_none());
        assert_eq!(chest_binding.container_slot_count(), Some(27));
        assert!(catalog.workstations().contains(&crafting));
        assert!(catalog.workstations().contains(&furnace));
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
