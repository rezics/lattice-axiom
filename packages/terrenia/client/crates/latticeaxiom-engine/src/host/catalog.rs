//! Package-authored catalogs consumed by the production host.
//!
//! Identities are loaded exclusively from lock-selected, receipt-verified data
//! realizations. Explicit source compilers remain available for tooling and
//! tests, but production never reads or embeds workspace package files.

use std::{
    collections::{BTreeMap, BTreeSet},
    num::{NonZeroU8, NonZeroU16, NonZeroU32},
    sync::Arc,
};

use latticeaxiom_compose::{LockedPackage, RealizedDataRootV1, RegistrationKind};
use latticeaxiom_content::{
    BiomeDefinitionV1, ContentCatalogInputV1, ContentCatalogLimitsV1, ContentCatalogV1,
    FluidDefinitionV1,
};
use latticeaxiom_core::{CanonicalLogicalPath, CapabilityId, PackageName, SchemaId, StableId};
use latticeaxiom_gameplay::{
    BlockDefinitionV1, BlockId, BlockSchemaBindingV1, CatalogLimits, FrozenItemRoleBindingV1,
    FuelRuleV1, GameplayCatalog, GameplayCatalogSourceV1, IngredientV1, ItemCategoryDefinitionV1,
    ItemCategoryId, ItemDefinitionV1, ItemId, ItemPredicateV1, ItemRoleDefinitionV1, ItemRoleId,
    ItemTagDefinitionV1, ItemTagId, MiningRuleV1, ProcessDefinitionV1, ProcessId,
    RecipeDefinitionV1, RecipeId, RecipePatternV1, RoleOutputV1, ToolClassId, ToolDefinitionV1,
    ToolRequirementV1, WorkstationDefinitionV1, WorkstationId,
};
use latticeaxiom_registration::CompiledRegistration;
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

/// Exactly-one terrain/worldgen provider selected by a reopened product lock.
pub(super) const WORLDGEN_TERRAIN_CAPABILITY: &str =
    "latticeaxiom:capability/worldgen-terrain-provider@2";
/// Exactly-one content-blocks provider selected by a reopened product lock.
pub(super) const CONTENT_BLOCKS_CAPABILITY: &str = "latticeaxiom:capability/content-blocks@1";
/// Exactly-one sandbox gameplay-rules provider selected by a reopened product lock.
pub(super) const SANDBOX_GAMEPLAY_CAPABILITY: &str = "latticeaxiom:capability/sandbox-gameplay@1";
/// Exactly-one sandbox tools provider selected by a reopened product lock.
pub(super) const SANDBOX_TOOLS_CAPABILITY: &str = "latticeaxiom:capability/sandbox-tools@1";

const BLOCKS_CATALOG_PATH: &str = "data/authored-catalog-v1.json";
const GAMEPLAY_RULES_PATH: &str = "data/authored-rules-v1.json";
const ITEM_BROWSER_PATH: &str = "data/authored-item-browser-v1.json";
const TOOLS_CATALOG_PATH: &str = "data/authored-tools-v1.json";
const WORLDGEN_BINDINGS_PATH: &str = "data/authored-block-bindings-v1.json";
const WORLDGEN_BIOMES_PATH: &str = "data/authored-biomes-v1.json";
const WORLDGEN_NATURAL_LAYERS_PATH: &str = "data/authored-natural-layers-v1.json";
const D7_BLOCK_IDS_PATH: &str = "data/goldens/d7-block-ids.txt";
const D7_BIOME_IDS_PATH: &str = "data/goldens/d7-biome-ids.txt";
const D7_NATURAL_ROLE_IDS_PATH: &str = "data/goldens/d7-natural-role-ids.txt";
const D9_BLOCK_IDS_PATH: &str = "data/goldens/d9-block-ids.txt";
const PACKAGE_PURPOSE_PATH: &str = "data/package-purpose-v1.json";

pub(super) fn required_provider_data(
    images: &LockVerifiedComposeImages,
    capability: &'static str,
) -> Result<(LockedPackage, Arc<RealizedDataRootV1>), ProductionHostError> {
    let package = exactly_one_lock_provider(images, capability)?.ok_or_else(|| {
        ProductionHostError::MissingLockProvider {
            capability: capability.to_owned(),
        }
    })?;
    let data = images.locked_artifacts().data_root(&package.name)?;
    Ok((package, data))
}

pub(super) fn required_data_file<'a>(
    data: &'a RealizedDataRootV1,
    logical_path: &'static str,
) -> Result<&'a [u8], ProductionHostError> {
    let path = CanonicalLogicalPath::new(logical_path).map_err(|_| {
        ProductionHostError::InvalidCatalogField {
            field: "locked-data-path",
        }
    })?;
    data.file(&path)
        .ok_or_else(|| ProductionHostError::MissingCatalogDefinition {
            kind: "locked-data-file",
            id: format!("{}:{logical_path}", data.package()),
        })
}

pub(super) fn required_data_text<'a>(
    data: &'a RealizedDataRootV1,
    logical_path: &'static str,
) -> Result<&'a str, ProductionHostError> {
    std::str::from_utf8(required_data_file(data, logical_path)?).map_err(|_| {
        ProductionHostError::InvalidCatalogField {
            field: "locked-data-utf8",
        }
    })
}

/// Explicit package-owned sources accepted by the gameplay compiler.
#[derive(Clone, Copy, Debug)]
pub struct AuthoredGameplayCatalogSourcesV1<'a> {
    /// Block definitions supplied by the content-blocks provider.
    pub blocks: &'a str,
    /// Sandbox rules supplied by the sandbox-gameplay provider.
    pub rules: &'a str,
    /// Primary item-browser categories supplied by the sandbox-gameplay provider.
    pub browser: &'a str,
    /// Tool definitions supplied by the sandbox-tools provider.
    pub tools: &'a str,
    /// Required D9 block identities supplied by the blocks provider.
    pub d9_block_ids: &'a str,
}

/// Explicit package-owned sources accepted by the content compiler.
#[derive(Clone, Copy, Debug)]
pub struct AuthoredContentCatalogSourcesV1<'a> {
    /// Block and fluid definitions supplied by the content-blocks provider.
    pub blocks: &'a str,
    /// Biome definitions supplied by the terrain/worldgen provider.
    pub biomes: &'a str,
    /// Required D7 biome identities supplied by the worldgen provider.
    pub d7_biome_ids: &'a str,
    /// Required D9 block identities supplied by the blocks provider.
    pub d9_block_ids: &'a str,
}

/// Worldgen identities compiled from the package catalog.
#[derive(Clone, Debug)]
pub(super) struct HostWorldgenCatalog {
    pub(super) dimension: DimensionId,
    pub(super) palette: Vec<BlockId>,
    pub(super) empty: BlockId,
    pub(super) placement_content: BlockId,
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
pub fn compile_authored_gameplay_catalog(
    sources: AuthoredGameplayCatalogSourcesV1<'_>,
) -> Result<GameplayCatalog, ProductionHostError> {
    let catalog =
        GameplayCatalog::compile(authored_gameplay_source(sources)?, CatalogLimits::default())
            .map_err(ProductionHostError::from)?;
    require_compiled_gameplay_covers_d9(&catalog, sources.blocks, sources.d9_block_ids)?;
    Ok(catalog)
}

/// Compiles the gameplay catalog selected by a reopened product lock.
///
/// Generic sandbox mechanics stay in [`GameplayCatalog`]. Concrete tool and
/// rule rows come from the lock-selected sandbox-tools and sandbox-gameplay
/// providers. Missing both providers yields an empty catalog so voxel-only
/// fixtures do not embed a hidden content fallback. Selecting only one of the
/// two required providers fails closed.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when the lock lists an empty or duplicate
/// provider, only one of the two sandbox capabilities is selected, or the
/// selected packages fail catalog compilation.
pub fn lock_selected_gameplay_catalog(
    images: &LockVerifiedComposeImages,
) -> Result<GameplayCatalog, ProductionHostError> {
    let gameplay = exactly_one_lock_provider(images, SANDBOX_GAMEPLAY_CAPABILITY)?;
    let tools = exactly_one_lock_provider(images, SANDBOX_TOOLS_CAPABILITY)?;
    match (gameplay.is_some(), tools.is_some()) {
        (true, true) => compile_lock_selected_gameplay_catalog(images),
        (false, false) => empty_gameplay_catalog(),
        (true, false) => Err(ProductionHostError::MissingLockProvider {
            capability: SANDBOX_TOOLS_CAPABILITY.to_owned(),
        }),
        (false, true) => Err(ProductionHostError::MissingLockProvider {
            capability: SANDBOX_GAMEPLAY_CAPABILITY.to_owned(),
        }),
    }
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
pub fn compile_authored_content_catalog(
    sources: AuthoredContentCatalogSourcesV1<'_>,
) -> Result<ContentCatalogV1, ProductionHostError> {
    let authored = parse_json_object(sources.blocks, "blocks-catalog")?;
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
    let biomes = authored_biome_definitions(sources.biomes)?;
    require_d7_golden_biome_ids(
        &biomes
            .iter()
            .map(|biome| biome.header.stable_id.clone())
            .collect(),
        sources.d7_biome_ids,
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
        sources.d9_block_ids,
    )?;
    Ok(catalog)
}

/// Compiles the content catalog selected by a reopened product lock.
///
/// # Errors
///
/// Returns [`ProductionHostError`] when a required provider or artifact is
/// absent, or package-owned JSON and golden identities fail validation.
pub fn lock_selected_content_catalog(
    images: &LockVerifiedComposeImages,
) -> Result<ContentCatalogV1, ProductionHostError> {
    let (_, blocks) = required_provider_data(images, CONTENT_BLOCKS_CAPABILITY)?;
    let (_, worldgen) = required_provider_data(images, WORLDGEN_TERRAIN_CAPABILITY)?;
    compile_authored_content_catalog(AuthoredContentCatalogSourcesV1 {
        blocks: required_data_text(&blocks, BLOCKS_CATALOG_PATH)?,
        biomes: required_data_text(&worldgen, WORLDGEN_BIOMES_PATH)?,
        d7_biome_ids: required_data_text(&worldgen, D7_BIOME_IDS_PATH)?,
        d9_block_ids: required_data_text(&blocks, D9_BLOCK_IDS_PATH)?,
    })
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
    let (worldgen_package, worldgen_data) =
        required_provider_data(images, WORLDGEN_TERRAIN_CAPABILITY)?;
    let (_, blocks_data) = required_provider_data(images, CONTENT_BLOCKS_CAPABILITY)?;
    let blocks_json = required_data_text(&blocks_data, BLOCKS_CATALOG_PATH)?;
    let bindings_json = required_data_text(&worldgen_data, WORLDGEN_BINDINGS_PATH)?;
    let natural_layers_json = required_data_text(&worldgen_data, WORLDGEN_NATURAL_LAYERS_PATH)?;
    let d7_block_ids = required_data_text(&blocks_data, D7_BLOCK_IDS_PATH)?;
    let d9_block_ids = required_data_text(&blocks_data, D9_BLOCK_IDS_PATH)?;
    let d7_natural_role_ids = required_data_text(&worldgen_data, D7_NATURAL_ROLE_IDS_PATH)?;
    let catalog_ids =
        authored_catalog_block_ids(&parse_json_object(blocks_json, "blocks-catalog")?)?;
    require_d7_golden_block_ids(&catalog_ids, d7_block_ids)?;
    require_d9_golden_block_ids(&catalog_ids, d9_block_ids)?;
    let bindings = AuthoredWorldgenBindingsV1::from_json(bindings_json.as_bytes())?;
    let authored: AuthoredBlockBindings =
        serde_json::from_str(bindings_json).map_err(|source| {
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
    require_d7_natural_role_ids(&role_bindings, d7_natural_role_ids)?;
    let empty = bound_block(&role_vocabulary, &role_bindings, D4MaterialRoleV1::Empty)?;
    let placement_content = bound_block(
        &role_vocabulary,
        &role_bindings,
        D4MaterialRoleV1::TemperateSubsurface,
    )?;
    for required in [&empty, &placement_content] {
        if !palette.iter().any(|block| block == required) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "palette-block",
                id: required.as_str().to_owned(),
            });
        }
    }

    Ok(HostWorldgenCatalog {
        dimension: resolve_dimension(images)?,
        palette,
        empty,
        placement_content,
        role_vocabulary,
        natural_vocabulary,
        role_bindings,
        block_catalog,
        cave: authored_cave_bindings(natural_layers_json)?,
        hydrology: authored_hydrology_bindings(&bindings, blocks_json)?,
        bindings,
        worldgen_package: Some(worldgen_package),
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
        config: HydrologyOccupancyConfigV1,
    ) -> Result<HydrologyOccupancyInputV1, ProductionHostError> {
        config.validate()?;
        Ok(HydrologyOccupancyInputV1::new(
            config,
            self.hydrology.fluids.clone(),
        ))
    }
}

fn authored_cave_bindings(
    natural_layers_json: &str,
) -> Result<HostCaveBindings, ProductionHostError> {
    let authored = parse_json_object(natural_layers_json, "natural-layers")?;
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
    blocks_json: &str,
) -> Result<HostHydrologyBindings, ProductionHostError> {
    let authored = parse_json_object(blocks_json, "blocks-catalog")?;
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
        CaveOwnedDomainV1, CaveTopologyLayerInputV1, cell_center_voxels_at_edge,
    };

    const TOPOLOGY_CELL_EDGE_VOXELS: u32 = 64;

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
            [2, 2],
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
        cell_center_voxels_at_edge(-2, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
        cell_center_voxels_at_edge(-1, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
        cell_center_voxels_at_edge(0, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
        cell_center_voxels_at_edge(1, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
    ];
    let second_path = [
        cell_center_voxels_at_edge(2, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
        cell_center_voxels_at_edge(1, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
        cell_center_voxels_at_edge(0, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
        cell_center_voxels_at_edge(-1, 0, y_mm, TOPOLOGY_CELL_EDGE_VOXELS),
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
    )?
    .with_cell_edge_voxels(TOPOLOGY_CELL_EDGE_VOXELS)?)
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

fn authored_biome_definitions(
    biomes_json: &str,
) -> Result<Vec<BiomeDefinitionV1>, ProductionHostError> {
    let authored = parse_json_object(biomes_json, "biome-catalog")?;
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

#[derive(Clone, Debug, Eq, PartialEq)]
struct ExactlyOneDimensionRegistration(StableId);

impl ExactlyOneDimensionRegistration {
    fn from_registration_image(
        registration: &CompiledRegistration,
    ) -> Result<Self, ProductionHostError> {
        let Some(table) = registration
            .image_receipt
            .numeric_ids
            .get(&RegistrationKind::Dimension)
        else {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "dimension-registration",
                id: "exactly-one".to_owned(),
            });
        };
        Self::from_registration_ids(table.keys())
    }

    /// Selects exactly one lock-selected realized `package-purpose` dimension.
    ///
    /// Production reopen still binds a placeholder empty registration image.
    /// Dimension identity therefore comes from receipt-verified data roots
    /// until the lock compiles a populated registration image.
    fn from_lock_selected_purposes(
        images: &LockVerifiedComposeImages,
    ) -> Result<Self, ProductionHostError> {
        let path = CanonicalLogicalPath::new(PACKAGE_PURPOSE_PATH).map_err(|_| {
            ProductionHostError::InvalidCatalogField {
                field: "package-purpose-path",
            }
        })?;
        let mut dimensions = BTreeSet::new();
        for name in images.locked_artifacts().data_package_names() {
            let data = images.locked_artifacts().data_root(name)?;
            let Some(bytes) = data.file(&path) else {
                continue;
            };
            let Some(purpose) = dimension_purpose_from_descriptor(name, bytes)? else {
                continue;
            };
            dimensions.insert(purpose);
        }
        Self::from_registration_ids(dimensions.iter())
    }

    fn from_registration_ids<'a>(
        ids: impl IntoIterator<Item = &'a StableId>,
    ) -> Result<Self, ProductionHostError> {
        let mut dimensions = ids.into_iter().filter(|id| id.kind() == "dimension");
        let Some(dimension) = dimensions.next() else {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "dimension-registration",
                id: "exactly-one".to_owned(),
            });
        };
        if dimensions.next().is_some() {
            return Err(ProductionHostError::AmbiguousDimension);
        }
        Ok(Self(dimension.clone()))
    }

    fn into_dimension(self) -> Result<DimensionId, ProductionHostError> {
        Ok(DimensionId::new(self.0)?)
    }
}

fn resolve_dimension(
    images: &LockVerifiedComposeImages,
) -> Result<DimensionId, ProductionHostError> {
    match ExactlyOneDimensionRegistration::from_registration_image(images.images().registration()) {
        Ok(selected) => selected.into_dimension(),
        Err(ProductionHostError::MissingCatalogDefinition {
            kind: "dimension-registration",
            ..
        }) => {
            ExactlyOneDimensionRegistration::from_lock_selected_purposes(images)?.into_dimension()
        }
        Err(error) => Err(error),
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
enum PackagePurposeSchemaV1 {
    #[serde(rename = "latticeaxiom.package-purpose.v1")]
    V1,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
enum PackagePurposeKindV1 {
    Capability,
    Dimension,
}

#[derive(Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
struct PackagePurposeDescriptorV1 {
    package: PackageName,
    purpose: StableId,
    purpose_kind: PackagePurposeKindV1,
    schema: PackagePurposeSchemaV1,
}

fn dimension_purpose_from_descriptor(
    package: &PackageName,
    bytes: &[u8],
) -> Result<Option<StableId>, ProductionHostError> {
    let descriptor: PackagePurposeDescriptorV1 =
        serde_json::from_slice(bytes).map_err(|source| {
            ProductionHostError::InvalidAuthoredCatalog {
                name: "package-purpose",
                source,
            }
        })?;
    if descriptor.package != *package {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "package-purpose-owner",
            id: descriptor.package.to_string(),
        });
    }
    if descriptor.purpose_kind != PackagePurposeKindV1::Dimension {
        return Ok(None);
    }
    if descriptor.purpose.kind() != "dimension" {
        return Err(ProductionHostError::MissingCatalogDefinition {
            kind: "dimension-registration",
            id: descriptor.purpose.to_string(),
        });
    }
    Ok(Some(descriptor.purpose))
}

fn d7_golden_block_ids(source: &str) -> Result<BTreeSet<StableId>, ProductionHostError> {
    golden_stable_ids(source, "block", "d7-block")
}

fn d9_golden_block_ids(source: &str) -> Result<BTreeSet<StableId>, ProductionHostError> {
    golden_stable_ids(source, "block", "d9-block")
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

fn require_d7_golden_biome_ids(
    present: &BTreeSet<StableId>,
    source: &str,
) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d7-biome",
        golden_stable_ids(source, "biome", "d7-biome")?
            .into_iter()
            .filter(|id| !present.contains(id))
            .map(|id| id.as_str().to_owned()),
    )
}

fn require_d7_golden_block_ids(
    present: &BTreeSet<StableId>,
    source: &str,
) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d7-block",
        d7_golden_block_ids(source)?
            .into_iter()
            .filter(|id| !present.contains(id))
            .map(|id| id.as_str().to_owned()),
    )
}

fn require_d9_golden_block_ids(
    present: &BTreeSet<StableId>,
    source: &str,
) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d9-block",
        d9_golden_block_ids(source)?
            .into_iter()
            .filter(|id| !present.contains(id))
            .map(|id| id.as_str().to_owned()),
    )
}

fn require_d7_natural_role_ids(
    bindings: &FrozenRoleBindingsV1,
    source: &str,
) -> Result<(), ProductionHostError> {
    missing_catalog_ids(
        "d7-natural-role",
        golden_stable_ids(source, "block-role", "d7-natural-role")?
            .into_iter()
            .filter(|id| bindings.target(id).is_none())
            .map(|id| id.as_str().to_owned()),
    )
}

fn require_compiled_gameplay_covers_d9(
    catalog: &GameplayCatalog,
    blocks_json: &str,
    d9_block_ids: &str,
) -> Result<(), ProductionHostError> {
    let blocks = parse_json_object(blocks_json, "blocks-catalog")?;
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
    for id in d9_golden_block_ids(d9_block_ids)? {
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

fn authored_gameplay_source(
    sources: AuthoredGameplayCatalogSourcesV1<'_>,
) -> Result<GameplayCatalogSourceV1, ProductionHostError> {
    let blocks = parse_json_object(sources.blocks, "blocks-catalog")?;
    require_d9_golden_block_ids(&authored_catalog_block_ids(&blocks)?, sources.d9_block_ids)?;
    let rules = parse_json_object(sources.rules, "gameplay-rules")?;
    let browser = parse_json_object(sources.browser, "item-browser")?;
    let tools = parse_json_object(sources.tools, "tools")?;
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
    let processes = ingest_processes(json_array(&rules, "processes")?, &mut workstations)?;
    let fuel_rules = ingest_fuel_rules(json_array(&rules, "fuel_rules")?)?;
    let tags = ingest_item_tags(json_array(&rules, "item_tags")?)?;
    let tool_requirements = tool_requirement_map(json_array(&rules, "tool_requirements")?)?;
    let mining_rules = mining_rule_ids(json_array(&rules, "mining_rules")?)?;
    let drop_tables = drop_table_map(json_array(&rules, "drop_tables")?, &tool_requirements)?;
    Ok(GameplayCatalogSourceV1 {
        items: items.into_values().collect(),
        blocks: compile_blocks(
            json_array(&blocks, "blocks")?,
            &drop_tables,
            &tool_requirements,
            &mining_rules,
        )?,
        tools: compile_tools(json_array(&tools, "tools")?)?,
        tags,
        categories: ingest_item_categories(json_array(&browser, "categories")?)?,
        roles: item_roles,
        bindings,
        recipes,
        workstations: workstations.into_values().collect(),
        processes,
        fuel_rules,
        block_schema_bindings,
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

fn ingest_item_categories(
    rows: &[Value],
) -> Result<Vec<ItemCategoryDefinitionV1>, ProductionHostError> {
    rows.iter()
        .map(|row| {
            let sort_order = u16::try_from(json_u64(row, "sort_order")?).map_err(|_| {
                ProductionHostError::InvalidCatalogField {
                    field: "sort_order",
                }
            })?;
            let members = json_array(row, "members")?
                .iter()
                .map(|member| {
                    member
                        .as_str()
                        .ok_or(ProductionHostError::InvalidCatalogField { field: "members" })
                        .and_then(|item| ItemId::parse(item).map_err(ProductionHostError::from))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Ok(ItemCategoryDefinitionV1 {
                id: ItemCategoryId::parse(json_text(row, "id")?)?,
                display_name: json_text(row, "display_name")?.to_owned(),
                icon: ItemId::parse(json_text(row, "icon")?)?,
                sort_order,
                members: members.into_boxed_slice(),
            })
        })
        .collect()
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

fn ingest_processes(
    rows: &[Value],
    workstations: &mut BTreeMap<String, WorkstationDefinitionV1>,
) -> Result<Vec<ProcessDefinitionV1>, ProductionHostError> {
    let mut processes = Vec::new();
    for row in rows {
        let workstation_id = json_text(row, "workstation")?;
        let workstation = WorkstationId::parse(workstation_id)?;
        workstations.insert(
            workstation_id.to_owned(),
            WorkstationDefinitionV1 {
                id: workstation.clone(),
            },
        );
        let input = json_object_field(row, "input")?;
        processes.push(ProcessDefinitionV1 {
            id: ProcessId::parse(json_text(row, "id")?)?,
            workstation,
            input: IngredientV1 {
                accepts: ItemPredicateV1::Exact(ItemId::parse(json_text(input, "item")?)?),
                quantity: json_quantity(input, "quantity")?,
            },
            output: RoleOutputV1 {
                role: ItemRoleId::parse(json_text(row, "output_role")?)?,
                quantity: json_quantity(&row["output"], "quantity")?,
            },
            duration_ticks: json_quantity(row, "duration_ticks")?,
        });
    }
    Ok(processes)
}

fn ingest_fuel_rules(rows: &[Value]) -> Result<Vec<FuelRuleV1>, ProductionHostError> {
    let mut rules = Vec::new();
    for row in rows {
        let accepts = json_object_field(row, "accepts")?;
        rules.push(FuelRuleV1 {
            accepts: ItemPredicateV1::Exact(ItemId::parse(json_text(accepts, "item")?)?),
            burn_ticks: json_quantity(row, "burn_ticks")?,
        });
    }
    Ok(rules)
}

fn ingest_item_tags(rows: &[Value]) -> Result<Vec<ItemTagDefinitionV1>, ProductionHostError> {
    let mut tags = Vec::new();
    for row in rows {
        let members = json_array(row, "members")?
            .iter()
            .map(|member| {
                let Some(id) = member.as_str() else {
                    return Err(ProductionHostError::InvalidCatalogField { field: "members" });
                };
                Ok(ItemId::parse(id)?)
            })
            .collect::<Result<Vec<_>, ProductionHostError>>()?;
        tags.push(ItemTagDefinitionV1 {
            id: ItemTagId::parse(json_text(row, "id")?)?,
            members: members.into_boxed_slice(),
        });
    }
    Ok(tags)
}

fn json_object_field<'a>(
    value: &'a Value,
    key: &'static str,
) -> Result<&'a Value, ProductionHostError> {
    match value.get(key) {
        Some(field) if field.is_object() => Ok(field),
        _ => Err(ProductionHostError::InvalidCatalogField { field: key }),
    }
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
    mining_rules: &BTreeSet<String>,
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
        let mining_rule_id = json_text(rules, "mining_rule")?;
        if !mining_rules.contains(mining_rule_id) {
            return Err(ProductionHostError::MissingCatalogDefinition {
                kind: "mining-rule",
                id: mining_rule_id.to_owned(),
            });
        }
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
    tool_requirements: &BTreeMap<String, Option<ToolRequirementV1>>,
) -> Result<BTreeMap<String, latticeaxiom_gameplay::ItemStackV1>, ProductionHostError> {
    let mut tables = BTreeMap::new();
    let mut ids = BTreeSet::new();
    for row in rows {
        let id = json_text(row, "id")?;
        if !ids.insert(id.to_owned()) {
            return Err(ProductionHostError::InvalidCatalogField {
                field: "duplicate-drop-table",
            });
        }
        for field in ["mining_speed_predicate", "drop_predicate"] {
            let requirement = json_text(row, field)?;
            if !tool_requirements.contains_key(requirement) {
                return Err(ProductionHostError::MissingCatalogDefinition {
                    kind: "tool-requirement",
                    id: requirement.to_owned(),
                });
            }
        }
        let outputs = json_array(row, "outputs")?;
        if outputs.len() != 1 {
            continue;
        }
        let item = json_text(&outputs[0], "item")?;
        let quantity = json_quantity(&outputs[0], "quantity")?;
        tables.insert(
            id.to_owned(),
            latticeaxiom_gameplay::ItemStackV1::plain(ItemId::parse(item)?, quantity.get())?,
        );
    }
    Ok(tables)
}

fn mining_rule_ids(rows: &[Value]) -> Result<BTreeSet<String>, ProductionHostError> {
    let mut rules = BTreeSet::new();
    for row in rows {
        if !rules.insert(json_text(row, "id")?.to_owned()) {
            return Err(ProductionHostError::InvalidCatalogField {
                field: "duplicate-mining-rule",
            });
        }
    }
    Ok(rules)
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
        if requirements
            .insert(json_text(row, "id")?.to_owned(), required)
            .is_some()
        {
            return Err(ProductionHostError::InvalidCatalogField {
                field: "duplicate-tool-requirement",
            });
        }
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

fn compile_lock_selected_gameplay_catalog(
    images: &LockVerifiedComposeImages,
) -> Result<GameplayCatalog, ProductionHostError> {
    let (_, blocks) = required_provider_data(images, CONTENT_BLOCKS_CAPABILITY)?;
    let (_, gameplay) = required_provider_data(images, SANDBOX_GAMEPLAY_CAPABILITY)?;
    let (_, tools) = required_provider_data(images, SANDBOX_TOOLS_CAPABILITY)?;
    compile_authored_gameplay_catalog(AuthoredGameplayCatalogSourcesV1 {
        blocks: required_data_text(&blocks, BLOCKS_CATALOG_PATH)?,
        rules: required_data_text(&gameplay, GAMEPLAY_RULES_PATH)?,
        browser: required_data_text(&gameplay, ITEM_BROWSER_PATH)?,
        tools: required_data_text(&tools, TOOLS_CATALOG_PATH)?,
        d9_block_ids: required_data_text(&blocks, D9_BLOCK_IDS_PATH)?,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use latticeaxiom_compose::RealizedDataRootV1;
    use latticeaxiom_core::{CanonicalLogicalPath, StableId};
    use serde_json::Value;

    use super::{
        AuthoredContentCatalogSourcesV1, AuthoredGameplayCatalogSourcesV1,
        authored_catalog_block_ids, compile_authored_content_catalog,
        compile_authored_gameplay_catalog, dimension_purpose_from_descriptor, parse_json_object,
    };
    use latticeaxiom_content::ContentCatalogV1;
    use latticeaxiom_gameplay::{
        BlockId, ContainerOwnerComponentV1, ContainerStateV1, FurnaceContinuationV1,
        GameplayCatalog, GameplayMutationIntentV1, ItemStackV1, SchemaId, WorkstationId,
    };
    use latticeaxiom_worldgen::D7_NATURAL_BLOCK_COUNT;
    const AUTHORED_BLOCKS_JSON: &str =
        include_str!("../../../../../blocks/data/authored-catalog-v1.json");
    const AUTHORED_RULES_JSON: &str =
        include_str!("../../../../../gameplay/data/authored-rules-v1.json");
    const AUTHORED_ITEM_BROWSER_JSON: &str =
        include_str!("../../../../../gameplay/data/authored-item-browser-v1.json");
    const AUTHORED_TOOLS_JSON: &str =
        include_str!("../../../../../tools/data/authored-tools-v1.json");
    const AUTHORED_BIOMES_JSON: &str =
        include_str!("../../../../../worldgen/data/authored-biomes-v1.json");
    const D7_BLOCK_IDS: &str = include_str!("../../../../../blocks/data/goldens/d7-block-ids.txt");
    const D7_BIOME_IDS: &str =
        include_str!("../../../../../worldgen/data/goldens/d7-biome-ids.txt");
    const D9_BLOCK_IDS: &str = include_str!("../../../../../blocks/data/goldens/d9-block-ids.txt");

    fn test_content_catalog() -> Result<ContentCatalogV1, super::ProductionHostError> {
        compile_authored_content_catalog(AuthoredContentCatalogSourcesV1 {
            blocks: AUTHORED_BLOCKS_JSON,
            biomes: AUTHORED_BIOMES_JSON,
            d7_biome_ids: D7_BIOME_IDS,
            d9_block_ids: D9_BLOCK_IDS,
        })
    }

    fn compile_gameplay_fixture(
        blocks: &str,
        rules: &str,
    ) -> Result<GameplayCatalog, super::ProductionHostError> {
        compile_authored_gameplay_catalog(AuthoredGameplayCatalogSourcesV1 {
            blocks,
            rules,
            browser: AUTHORED_ITEM_BROWSER_JSON,
            tools: AUTHORED_TOOLS_JSON,
            d9_block_ids: D9_BLOCK_IDS,
        })
    }

    fn test_gameplay_catalog() -> Result<GameplayCatalog, super::ProductionHostError> {
        compile_gameplay_fixture(AUTHORED_BLOCKS_JSON, AUTHORED_RULES_JSON)
    }

    fn fixture_registration_ids(ids: &[&str]) -> Vec<StableId> {
        ids.iter()
            .map(|id| {
                id.parse::<StableId>()
                    .unwrap_or_else(|error| panic!("fixture registration `{id}` is valid: {error}"))
            })
            .collect()
    }

    #[test]
    fn dimension_registration_fails_closed_when_absent() {
        let registrations = fixture_registration_ids(&["fixture:block/air"]);
        let error =
            super::ExactlyOneDimensionRegistration::from_registration_ids(registrations.iter())
                .expect_err("a fixture must explicitly register its dimension");
        assert!(matches!(
            error,
            super::ProductionHostError::MissingCatalogDefinition {
                kind: "dimension-registration",
                ref id,
            } if id == "exactly-one"
        ));
    }

    #[test]
    fn dimension_registration_accepts_one_typed_registration() {
        let registrations =
            fixture_registration_ids(&["fixture:block/air", "terrenia:dimension/terrenia"]);
        let dimension =
            super::ExactlyOneDimensionRegistration::from_registration_ids(registrations.iter())
                .expect("exactly one dimension registration proves selection")
                .into_dimension()
                .expect("the typed registration converts to a dimension ID");
        assert_eq!(dimension.as_str(), "terrenia:dimension/terrenia");
    }

    #[test]
    fn dimension_registration_fails_closed_when_duplicate() {
        let registrations =
            fixture_registration_ids(&["terrenia:dimension/terrenia", "substitute:dimension/sky"]);
        let error =
            super::ExactlyOneDimensionRegistration::from_registration_ids(registrations.iter())
                .expect_err("multiple dimension registrations are ambiguous");
        assert!(matches!(
            error,
            super::ProductionHostError::AmbiguousDimension
        ));
    }

    #[test]
    fn dimension_registration_accepts_non_terrenia_substitute_provider() {
        let registrations =
            fixture_registration_ids(&["substitute:block/air", "substitute:dimension/sky"]);
        let dimension =
            super::ExactlyOneDimensionRegistration::from_registration_ids(registrations.iter())
                .expect("a substitute provider may own the sole dimension registration")
                .into_dimension()
                .expect("the substitute registration converts to a dimension ID");
        assert_eq!(dimension.as_str(), "substitute:dimension/sky");
    }

    #[test]
    fn shipped_terrenia_package_purpose_selects_the_dimension() {
        let package = "terrenia"
            .parse()
            .expect("the Terrenia package name is canonical");
        let purpose = dimension_purpose_from_descriptor(
            &package,
            include_bytes!("../../../../../main/data/package-purpose-v1.json"),
        )
        .expect("the shipped purpose document is valid")
        .expect("the Terrenia root purpose is a dimension");
        assert_eq!(purpose.as_str(), "terrenia:dimension/terrenia");
        super::ExactlyOneDimensionRegistration::from_registration_ids([&purpose])
            .expect("one realized dimension purpose is exactly-one")
            .into_dimension()
            .expect("the realized purpose converts to a dimension ID");
    }

    #[test]
    fn capability_package_purpose_is_not_a_dimension() {
        let package = "@latticeaxiom/observability"
            .parse()
            .expect("the observability package name is canonical");
        let purpose = dimension_purpose_from_descriptor(
            &package,
            include_bytes!(
                "../../../../../../latticeaxiom/observability/data/package-purpose-v1.json"
            ),
        )
        .expect("capability purpose documents remain valid");
        assert_eq!(purpose, None);
    }

    #[test]
    fn package_purpose_owner_mismatch_fails_closed() {
        let package = "@terrenia/blocks"
            .parse()
            .expect("the blocks package name is canonical");
        let error = dimension_purpose_from_descriptor(
            &package,
            include_bytes!("../../../../../main/data/package-purpose-v1.json"),
        )
        .expect_err("a purpose document cannot change its owning package");
        assert!(matches!(
            error,
            super::ProductionHostError::MissingCatalogDefinition {
                kind: "package-purpose-owner",
                ref id,
            } if id == "terrenia"
        ));
    }

    #[test]
    fn d7_golden_block_ids_are_present_in_authored_catalog() {
        let authored = authored_catalog_block_ids(
            &parse_json_object(AUTHORED_BLOCKS_JSON, "blocks-catalog")
                .expect("Terrenia authored catalog is valid JSON"),
        )
        .expect("authored catalog block IDs are valid");
        let golden = super::d7_golden_block_ids(D7_BLOCK_IDS).expect("D7 golden IDs are valid");
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
        let catalog = test_content_catalog().expect("package content catalog must compile");
        let compiled = catalog
            .biomes()
            .iter()
            .map(|biome| biome.header.stable_id.clone())
            .collect::<BTreeSet<_>>();
        let golden = super::golden_stable_ids(D7_BIOME_IDS, "biome", "d7-biome")
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
        let golden = super::d9_golden_block_ids(D9_BLOCK_IDS).expect("D9 golden IDs are valid");
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
        let catalog = test_gameplay_catalog().expect("package gameplay catalog must compile");
        let golden = super::d9_golden_block_ids(D9_BLOCK_IDS).expect("D9 golden IDs are valid");
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
    fn gameplay_compiler_fails_closed_for_dangling_mining_rule() {
        let mut blocks: Value = serde_json::from_str(AUTHORED_BLOCKS_JSON)
            .expect("Terrenia authored blocks are valid JSON");
        let row = blocks["blocks"]
            .as_array_mut()
            .expect("the authored block catalog has block rows")
            .iter_mut()
            .find(|row| {
                row.pointer("/physical/hardness_ticks")
                    .and_then(Value::as_u64)
                    .is_some_and(|ticks| ticks > 0)
            })
            .expect("the authored block catalog has a mineable block");
        let missing_id = "fixture:mining-rule/missing@1";
        *row.pointer_mut("/definition/rules/mining_rule")
            .expect("mineable blocks declare a mining rule") = Value::String(missing_id.to_owned());
        let blocks = serde_json::to_string(&blocks).expect("the mutated catalog serializes");

        let error = compile_gameplay_fixture(&blocks, AUTHORED_RULES_JSON)
            .expect_err("a dangling mining-rule reference must fail closed");
        assert!(matches!(
            error,
            super::ProductionHostError::MissingCatalogDefinition {
                kind: "mining-rule",
                ref id,
            } if id == missing_id
        ));
    }

    #[test]
    fn gameplay_compiler_fails_closed_for_dangling_drop_predicate() {
        let mut rules: Value = serde_json::from_str(AUTHORED_RULES_JSON)
            .expect("Terrenia authored gameplay rules are valid JSON");
        let row = rules["drop_tables"]
            .as_array_mut()
            .expect("the authored gameplay rules have drop tables")
            .first_mut()
            .expect("the authored gameplay rules have at least one drop table");
        let missing_id = "fixture:tool-requirement/missing@1";
        row["drop_predicate"] = Value::String(missing_id.to_owned());
        let rules = serde_json::to_string(&rules).expect("the mutated rules serialize");

        let error = compile_gameplay_fixture(AUTHORED_BLOCKS_JSON, &rules)
            .expect_err("a dangling drop predicate must fail closed");
        assert!(matches!(
            error,
            super::ProductionHostError::MissingCatalogDefinition {
                kind: "tool-requirement",
                ref id,
            } if id == missing_id
        ));
    }

    #[test]
    fn gameplay_compiler_rejects_duplicate_rule_definitions() {
        for (collection, expected_field) in [
            ("mining_rules", "duplicate-mining-rule"),
            ("tool_requirements", "duplicate-tool-requirement"),
            ("drop_tables", "duplicate-drop-table"),
        ] {
            let mut rules: Value = serde_json::from_str(AUTHORED_RULES_JSON)
                .expect("Terrenia authored gameplay rules are valid JSON");
            let rows = rules[collection]
                .as_array_mut()
                .unwrap_or_else(|| panic!("{collection} is an authored array"));
            let duplicate = rows
                .first()
                .cloned()
                .unwrap_or_else(|| panic!("{collection} has an authored row"));
            rows.push(duplicate);
            let rules = serde_json::to_string(&rules).expect("the mutated rules serialize");

            let error = compile_gameplay_fixture(AUTHORED_BLOCKS_JSON, &rules)
                .expect_err("a duplicate authored definition must fail closed");
            match error {
                super::ProductionHostError::InvalidCatalogField { field } => {
                    assert_eq!(field, expected_field, "{collection}");
                }
                other => panic!(
                    "{collection} produced the wrong error; expected {expected_field}, got {other}"
                ),
            }
        }
    }

    #[test]
    fn package_item_browser_assigns_one_primary_category_to_every_item() {
        let catalog = test_gameplay_catalog().expect("package gameplay catalog must compile");
        assert_eq!(catalog.categories().len(), 6);
        let uncategorized = catalog
            .items()
            .keys()
            .filter(|item| catalog.category_for_item(item).is_none())
            .map(ToString::to_string)
            .collect::<Vec<_>>();
        assert!(
            uncategorized.is_empty(),
            "every shipped item needs one primary browser category: {uncategorized:?}"
        );
    }

    #[test]
    fn d9_workbench_furnace_chest_and_torch_bind_reserved_gameplay_schemas() {
        let catalog = test_gameplay_catalog().expect("package gameplay catalog must compile");
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
        let catalog = test_content_catalog().expect("package content catalog must compile");
        let golden = super::d9_golden_block_ids(D9_BLOCK_IDS).expect("D9 golden IDs are valid");
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

    #[test]
    fn locked_data_text_fails_closed_for_missing_and_non_utf8_files() {
        let missing = RealizedDataRootV1::from_file_bytes(
            "fixture".parse().expect("fixture package is canonical"),
            CanonicalLogicalPath::new("data").expect("fixture root is canonical"),
            BTreeMap::from([("data/other.json".to_owned(), b"{}".to_vec())]),
        )
        .expect("missing-file fixture is otherwise valid");
        let Err(missing_error) = super::required_data_text(&missing, super::BLOCKS_CATALOG_PATH)
        else {
            panic!("missing locked data file must fail closed");
        };
        assert!(matches!(
            missing_error,
            super::ProductionHostError::MissingCatalogDefinition {
                kind: "locked-data-file",
                ..
            }
        ));

        let invalid_utf8 = RealizedDataRootV1::from_file_bytes(
            "fixture".parse().expect("fixture package is canonical"),
            CanonicalLogicalPath::new("data").expect("fixture root is canonical"),
            BTreeMap::from([(super::BLOCKS_CATALOG_PATH.to_owned(), vec![0xff])]),
        )
        .expect("non-UTF-8 fixture is otherwise valid");
        let Err(utf8_error) = super::required_data_text(&invalid_utf8, super::BLOCKS_CATALOG_PATH)
        else {
            panic!("non-UTF-8 locked data file must fail closed");
        };
        assert!(matches!(
            utf8_error,
            super::ProductionHostError::InvalidCatalogField {
                field: "locked-data-utf8"
            }
        ));
    }

    #[test]
    fn authored_compiler_fails_closed_for_invalid_json() {
        let Err(error) = compile_authored_content_catalog(AuthoredContentCatalogSourcesV1 {
            blocks: "{",
            biomes: AUTHORED_BIOMES_JSON,
            d7_biome_ids: D7_BIOME_IDS,
            d9_block_ids: D9_BLOCK_IDS,
        }) else {
            panic!("invalid package JSON must fail closed");
        };
        assert!(matches!(
            error,
            super::ProductionHostError::InvalidAuthoredCatalog {
                name: "blocks-catalog",
                ..
            }
        ));
    }
}
