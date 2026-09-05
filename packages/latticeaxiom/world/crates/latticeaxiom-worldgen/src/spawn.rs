//! Deterministic safe surface spawn selection for the D4/D7 contracts.
//!
//! Selection reuses [`GenerationPlanV1`] territory, height, density, and cave
//! queries. It does not compile a second Atlas, open a writer, or extend cave
//! topology. Package JSON supplies Role and Predicate identities; occupancy
//! drafts and ready-chunk membership are caller-supplied receipts.

use std::{
    collections::{BTreeMap, BTreeSet},
    str::FromStr,
};

use latticeaxiom_core::StableId;
use latticeaxiom_storage::ChunkCoordinate;
use serde::Deserialize;

use crate::{
    BoundedGeneratedRegionV1, ChunkDraftV1, D4BlockCatalogClosureV1, D4MaterialRoleV1,
    D4RoleVocabularyV1, FrozenRoleBindingsV1, GenerationPlanV1, TerrainStyleV1, WorldgenError,
    WorldgenResult, hashes::hash_u64,
};

const SPAWN_COLUMN_DOMAIN: &[u8] = b"latticeaxiom.spawn-column.v1\0";
const AUTHORED_BINDINGS_SCHEMA_PATH: &str = "schema/authored-worldgen-block-bindings@1";
const DEFAULT_CLEARANCE_VOXELS: u16 = 2;
const DEFAULT_SEARCH_HALF_EXTENT: i64 = 8;

const REQUIRED_PREDICATE_PATHS: [&str; 6] = [
    "place-empty",
    "place-solid",
    "place-surface",
    "place-water",
    "place-lava",
    "fluid-replaceable",
];

/// Closed reject kinds for one spawn cell or column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpawnRejectV1 {
    /// Standing or footing voxels contain a fluid occupancy.
    Fluid,
    /// The cell is a cave void or underground cave occupancy.
    Cave,
    /// Lava, cactus, or another authored hazard occupies the column.
    Hazard,
    /// The surface cell is not solid footing.
    MissingFooting,
    /// Player clearance voxels are not empty.
    InsufficientClearance,
}

impl SpawnRejectV1 {
    /// Returns the stable diagnostic label.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Fluid => "fluid",
            Self::Cave => "cave",
            Self::Hazard => "hazard",
            Self::MissingFooting => "missing-footing",
            Self::InsufficientClearance => "insufficient-clearance",
        }
    }
}

impl std::fmt::Display for SpawnRejectV1 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// Package-authored Role and Predicate bindings for worldgen and spawn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AuthoredWorldgenBindingsV1 {
    roles: BTreeMap<String, (StableId, StableId)>,
    predicates: BTreeMap<String, StableId>,
}

impl AuthoredWorldgenBindingsV1 {
    /// Decodes and validates authored worldgen Role/Predicate JSON.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidAuthoredBindings`] for malformed JSON,
    /// unknown schema, missing D4 Role purposes, or missing spawn Predicates.
    pub fn from_json(bytes: &[u8]) -> WorldgenResult<Self> {
        let document: AuthoredBindingsDocumentV1 =
            serde_json::from_slice(bytes).map_err(|error| {
                WorldgenError::InvalidAuthoredBindings {
                    field: "document",
                    reason: error.to_string(),
                }
            })?;
        let schema_path = document
            .authoring_schema
            .split_once(':')
            .map_or("", |(_, path)| path);
        if schema_path != AUTHORED_BINDINGS_SCHEMA_PATH {
            return Err(invalid_bindings(
                "authoring_schema",
                format!(
                    "expected a namespace-qualified `{AUTHORED_BINDINGS_SCHEMA_PATH}` schema, got `{}`",
                    document.authoring_schema
                ),
            ));
        }
        if document.roles.is_empty() {
            return Err(invalid_bindings(
                "roles",
                "at least one Role binding is required",
            ));
        }

        let mut roles = BTreeMap::new();
        for row in document.roles {
            let role = parse_identity("roles.id", &row.id, "block-role")?;
            let candidate = parse_identity("roles.candidate", &row.candidate, "block")?;
            if row.cardinality != "exactly-one" {
                return Err(invalid_bindings(
                    "roles.cardinality",
                    format!("Role `{}` must be exactly-one", row.id),
                ));
            }
            let path = role.path().to_owned();
            if roles.insert(path, (role, candidate)).is_some() {
                return Err(invalid_bindings(
                    "roles.id",
                    format!("duplicate Role path in `{}`", row.id),
                ));
            }
        }

        let mut predicates = BTreeMap::new();
        for row in document.predicates {
            let predicate = parse_identity("predicates.id", &row.id, "predicate")?;
            let path = predicate.path().to_owned();
            if predicates.insert(path, predicate).is_some() {
                return Err(invalid_bindings(
                    "predicates.id",
                    format!("duplicate Predicate path in `{}`", row.id),
                ));
            }
        }
        for purpose in D4MaterialRoleV1::ALL {
            if !roles.contains_key(purpose.as_str()) {
                return Err(WorldgenError::MissingRolePurpose {
                    purpose: purpose.as_str(),
                });
            }
        }
        for path in REQUIRED_PREDICATE_PATHS {
            if !predicates.contains_key(path) {
                return Err(invalid_bindings(
                    "predicates",
                    format!("missing required Predicate `{path}`"),
                ));
            }
        }
        Ok(Self { roles, predicates })
    }

    /// Returns the package-owned D4 Role vocabulary.
    ///
    /// # Errors
    ///
    /// Returns a Role-vocabulary error if a required purpose is missing.
    pub fn d4_vocabulary(&self) -> WorldgenResult<D4RoleVocabularyV1> {
        let entries = D4MaterialRoleV1::ALL.into_iter().map(|purpose| {
            let (role, _) = self.role_entry(purpose);
            (purpose, role.clone())
        });
        D4RoleVocabularyV1::new(entries)
    }

    /// Returns the package-owned D7 natural Role vocabulary.
    ///
    /// # Errors
    ///
    /// Returns a Role-vocabulary error if a required natural purpose is missing.
    pub fn natural_vocabulary(&self) -> WorldgenResult<crate::NaturalRoleVocabularyV1> {
        let mut entries = Vec::with_capacity(D4MaterialRoleV1::NATURAL.len());
        for purpose in D4MaterialRoleV1::NATURAL {
            let path = purpose.authored_catalog_path().unwrap_or(purpose.as_str());
            let Some((role, _)) = self.roles.get(path) else {
                return Err(WorldgenError::MissingRolePurpose {
                    purpose: purpose.as_str(),
                });
            };
            entries.push((purpose, role.clone()));
        }
        crate::NaturalRoleVocabularyV1::new(entries)
    }

    /// Returns frozen Role-to-block bindings for every authored Role row.
    ///
    /// # Errors
    ///
    /// Returns a frozen-binding error for duplicate or invalid identities.
    pub fn role_bindings(&self) -> WorldgenResult<FrozenRoleBindingsV1> {
        FrozenRoleBindingsV1::new(
            self.roles
                .values()
                .map(|(role, target)| (role.clone(), target.clone())),
        )
    }

    /// Returns unique authored block candidates as a D4 catalog closure.
    ///
    /// # Errors
    ///
    /// Returns a catalog-closure error when unique candidates fall below the
    /// D4 minimum or exceed the hard catalog bound.
    pub fn catalog_closure(&self) -> WorldgenResult<D4BlockCatalogClosureV1> {
        let mut blocks = BTreeSet::new();
        for (_, target) in self.roles.values() {
            blocks.insert(target.clone());
        }
        D4BlockCatalogClosureV1::new(blocks)
    }

    /// Returns the authored Predicate identity for a registration path.
    #[must_use]
    pub fn predicate(&self, path: &str) -> Option<&StableId> {
        self.predicates.get(path)
    }

    /// Returns the cactus hazard block when the catalog Role is authored.
    #[must_use]
    pub fn cactus_block(&self) -> Option<&StableId> {
        self.roles.get("catalog-cactus").map(|(_, target)| target)
    }

    fn role_entry(&self, purpose: D4MaterialRoleV1) -> &(StableId, StableId) {
        self.roles
            .get(purpose.as_str())
            .unwrap_or_else(|| unreachable_authored_role(purpose))
    }
}

fn unreachable_authored_role(purpose: D4MaterialRoleV1) -> &'static (StableId, StableId) {
    panic!(
        "validated AuthoredWorldgenBindingsV1 lost required purpose `{}`",
        purpose.as_str()
    )
}

/// Caller-supplied occupancy receipts used by spawn safety checks.
///
/// Missing chunk drafts stay unready. This view never synthesizes activation
/// evidence and never opens a writer.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct SpawnOccupancyViewV1 {
    drafts: BTreeMap<ChunkCoordinate, ChunkDraftV1>,
    overlays: BTreeMap<(i64, i64, i64), SpawnCellOverrideV1>,
}

impl SpawnOccupancyViewV1 {
    /// Creates an empty occupancy view with no ready chunks.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Marks a generated chunk ready by storing its role-resolved draft.
    pub fn insert_ready_draft(&mut self, coordinate: ChunkCoordinate, draft: ChunkDraftV1) {
        self.drafts.insert(coordinate, draft);
    }

    /// Records a test or hydrology overlay that does not alter generated drafts.
    pub fn overlay(&mut self, x: i64, y: i64, z: i64, cell: SpawnCellOverrideV1) {
        self.overlays.insert((x, y, z), cell);
    }

    /// Treats every prepared snapshot candidate as a ready chunk receipt.
    #[must_use]
    pub fn from_generated_region(region: &BoundedGeneratedRegionV1) -> Self {
        let mut occupancy = Self::new();
        for (coordinate, candidate) in region.candidates() {
            occupancy.insert_ready_draft(coordinate, candidate.draft().clone());
        }
        occupancy
    }

    /// Returns whether a generation receipt is present for `coordinate`.
    #[must_use]
    pub fn is_ready(&self, coordinate: ChunkCoordinate) -> bool {
        self.drafts.contains_key(&coordinate)
    }
}

/// Explicit occupancy overlay for fluid, cave, or hazard cells.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnCellOverrideV1 {
    fluid: bool,
    cave: bool,
    hazard: bool,
}

impl SpawnCellOverrideV1 {
    /// Marks a cell as unsafe fluid occupancy.
    #[must_use]
    pub const fn fluid() -> Self {
        Self {
            fluid: true,
            cave: false,
            hazard: false,
        }
    }

    /// Marks a cell as an unsafe cave void.
    #[must_use]
    pub const fn cave() -> Self {
        Self {
            fluid: false,
            cave: true,
            hazard: false,
        }
    }

    /// Marks lava: fluid occupancy that is also a hazard.
    #[must_use]
    pub const fn lava() -> Self {
        Self {
            fluid: true,
            cave: false,
            hazard: true,
        }
    }
}

/// Inclusive-exclusive XZ search window and player clearance in voxels.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnSearchBoundsV1 {
    min_x: i64,
    max_x: i64,
    min_z: i64,
    max_z: i64,
    clearance_voxels: u16,
}

impl SpawnSearchBoundsV1 {
    /// Creates a bounded search window.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenError::InvalidConfig`] when the window is empty or
    /// clearance is zero.
    pub fn new(
        min_x: i64,
        max_x: i64,
        min_z: i64,
        max_z: i64,
        clearance_voxels: u16,
    ) -> WorldgenResult<Self> {
        if min_x >= max_x || min_z >= max_z {
            return Err(WorldgenError::InvalidConfig {
                field: "spawn_search_bounds",
                reason: "XZ window must be non-empty".to_owned(),
            });
        }
        if clearance_voxels == 0 {
            return Err(WorldgenError::InvalidConfig {
                field: "clearance_voxels",
                reason: "player clearance must be at least one voxel".to_owned(),
            });
        }
        Ok(Self {
            min_x,
            max_x,
            min_z,
            max_z,
            clearance_voxels,
        })
    }

    /// Returns the default origin-neighborhood search used by V5 spawn.
    #[must_use]
    pub fn origin_neighborhood() -> Self {
        Self {
            min_x: -DEFAULT_SEARCH_HALF_EXTENT,
            max_x: DEFAULT_SEARCH_HALF_EXTENT,
            min_z: -DEFAULT_SEARCH_HALF_EXTENT,
            max_z: DEFAULT_SEARCH_HALF_EXTENT,
            clearance_voxels: DEFAULT_CLEARANCE_VOXELS,
        }
    }

    /// Returns the player standing-voxel count above footing.
    #[must_use]
    pub const fn clearance_voxels(self) -> u16 {
        self.clearance_voxels
    }
}

impl Default for SpawnSearchBoundsV1 {
    fn default() -> Self {
        Self::origin_neighborhood()
    }
}

/// Validated surface spawn footing in Bevy-native Y-up voxel coordinates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnLocationV1 {
    footing: [i64; 3],
    chunk: ChunkCoordinate,
    style: TerrainStyleV1,
}

impl SpawnLocationV1 {
    /// Returns the solid footing voxel `(x, y, z)`.
    #[must_use]
    pub const fn footing(self) -> [i64; 3] {
        self.footing
    }

    /// Returns the empty feet voxel immediately above footing.
    #[must_use]
    pub const fn feet(self) -> [i64; 3] {
        [
            self.footing[0],
            self.footing[1].saturating_add(1),
            self.footing[2],
        ]
    }

    /// Returns the chunk that contains the footing voxel.
    #[must_use]
    pub const fn chunk(self) -> ChunkCoordinate {
        self.chunk
    }

    /// Returns the D4/D7 surface style at the spawn column.
    #[must_use]
    pub const fn style(self) -> TerrainStyleV1 {
        self.style
    }
}

/// Occupancy class of one ready spawn cell.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SpawnOccupancyClassV1 {
    Empty,
    Passable,
    Solid,
    Fluid,
    Cave,
    Hazard,
    Lava,
    Blocked,
}

/// Classified occupancy of one world cell for spawn safety.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SpawnCellInspectionV1 {
    class: SpawnOccupancyClassV1,
}

impl SpawnCellInspectionV1 {
    /// Returns whether the containing chunk has a generation receipt.
    #[must_use]
    pub const fn is_ready(self) -> bool {
        true
    }

    /// Returns whether the cell is a cave void.
    #[must_use]
    pub const fn is_cave(self) -> bool {
        matches!(self.class, SpawnOccupancyClassV1::Cave)
    }

    /// Returns whether the cell has fluid occupancy.
    #[must_use]
    pub const fn is_fluid(self) -> bool {
        matches!(
            self.class,
            SpawnOccupancyClassV1::Fluid | SpawnOccupancyClassV1::Lava
        )
    }

    /// Returns whether the cell is an authored hazard.
    #[must_use]
    pub const fn is_hazard(self) -> bool {
        matches!(
            self.class,
            SpawnOccupancyClassV1::Hazard | SpawnOccupancyClassV1::Lava
        )
    }

    /// Returns whether the cell is solid footing.
    #[must_use]
    pub const fn is_solid(self) -> bool {
        matches!(self.class, SpawnOccupancyClassV1::Solid)
    }

    /// Returns whether the cell is empty clearance.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        matches!(self.class, SpawnOccupancyClassV1::Empty)
    }

    /// Returns whether the cell is clear for the player capsule.
    #[must_use]
    pub const fn is_clearance(self) -> bool {
        matches!(
            self.class,
            SpawnOccupancyClassV1::Empty | SpawnOccupancyClassV1::Passable
        )
    }
}

/// Unique chunks that must be ready before the bounded search can succeed.
///
/// # Errors
///
/// Returns an arithmetic error when a world voxel is outside the `i32` chunk
/// domain.
pub fn required_spawn_chunks(
    plan: &GenerationPlanV1,
    bounds: SpawnSearchBoundsV1,
) -> WorldgenResult<Vec<ChunkCoordinate>> {
    let edge = plan.config().chunk_edge_voxels;
    let clearance = i64::from(bounds.clearance_voxels);
    let mut chunks = BTreeSet::new();
    let mut z = bounds.min_z;
    while z < bounds.max_z {
        let mut x = bounds.min_x;
        while x < bounds.max_x {
            let height = i64::from(plan.terrain_height(x, z));
            let max_y = height.saturating_add(clearance);
            let mut y = height;
            while y <= max_y {
                chunks.insert(world_chunk(x, y, z, edge)?);
                y = y.saturating_add(1);
            }
            x = x.saturating_add(1);
        }
        z = z.saturating_add(1);
    }
    Ok(chunks.into_iter().collect())
}

/// Inspects one world cell using plan queries, ready drafts, and overlays.
///
/// # Errors
///
/// Returns [`WorldgenError::UnreadySpawnChunk`] when the cell's chunk has no
/// generation receipt, and an arithmetic error at the persistent chunk boundary.
pub fn inspect_spawn_cell(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    occupancy: &SpawnOccupancyViewV1,
    x: i64,
    y: i64,
    z: i64,
) -> WorldgenResult<SpawnCellInspectionV1> {
    let edge = plan.config().chunk_edge_voxels;
    let chunk = world_chunk(x, y, z, edge)?;
    let Some(draft) = occupancy.drafts.get(&chunk) else {
        return Err(WorldgenError::UnreadySpawnChunk { coordinate: chunk });
    };
    let (local_x, local_y, local_z) = local_voxel(x, y, z, edge)?;
    let block =
        draft
            .block_at(local_x, local_y, local_z)
            .ok_or(WorldgenError::ArithmeticOverflow {
                operation: "spawn draft lookup",
            })?;
    let overlay = occupancy.overlays.get(&(x, y, z)).copied();
    let height = i64::from(plan.terrain_height(x, z));
    let cave =
        overlay.is_some_and(|cell| cell.cave) || (y <= height && plan.terrain_density(x, y, z) < 0);
    let river = plan
        .river_sample(x, z)
        .is_some_and(|sample| sample.in_channel() && y <= height);
    let fluid = overlay.is_some_and(|cell| cell.fluid) || river;
    let hazard = overlay.is_some_and(|cell| cell.hazard)
        || river
        || bindings
            .cactus_block()
            .is_some_and(|cactus| block == cactus);
    let empty_block = plan.role_target(D4MaterialRoleV1::Empty);
    let passable_ground_cover = plan.role_target(D4MaterialRoleV1::WoodlandGroundCover);
    let class = if cave {
        SpawnOccupancyClassV1::Cave
    } else if fluid && hazard {
        SpawnOccupancyClassV1::Lava
    } else if fluid {
        SpawnOccupancyClassV1::Fluid
    } else if hazard {
        SpawnOccupancyClassV1::Hazard
    } else if block == empty_block {
        SpawnOccupancyClassV1::Empty
    } else if block == passable_ground_cover {
        SpawnOccupancyClassV1::Passable
    } else if plan.terrain_density(x, y, z) >= 0 {
        SpawnOccupancyClassV1::Solid
    } else {
        SpawnOccupancyClassV1::Blocked
    };
    Ok(SpawnCellInspectionV1 { class })
}

/// Validates one surface column for spawn safety.
///
/// Checks run in order: active-chunk readiness, fluid, cave, hazard, footing,
/// then clearance. The first failure is returned.
///
/// # Errors
///
/// Returns [`WorldgenError::UnreadySpawnChunk`] or
/// [`WorldgenError::UnsafeSpawnCell`] when a required check fails, and an
/// arithmetic error at the persistent chunk boundary.
pub fn evaluate_spawn_column(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    occupancy: &SpawnOccupancyViewV1,
    x: i64,
    z: i64,
    clearance_voxels: u16,
) -> WorldgenResult<SpawnLocationV1> {
    let _ = bindings
        .predicate("place-empty")
        .ok_or_else(|| invalid_bindings("predicates", "missing `place-empty`"))?;
    let _ = bindings
        .predicate("place-solid")
        .ok_or_else(|| invalid_bindings("predicates", "missing `place-solid`"))?;
    let _ = bindings
        .predicate("place-surface")
        .ok_or_else(|| invalid_bindings("predicates", "missing `place-surface`"))?;
    let _ = bindings
        .predicate("place-water")
        .ok_or_else(|| invalid_bindings("predicates", "missing `place-water`"))?;
    let _ = bindings
        .predicate("place-lava")
        .ok_or_else(|| invalid_bindings("predicates", "missing `place-lava`"))?;
    let _ = bindings
        .predicate("fluid-replaceable")
        .ok_or_else(|| invalid_bindings("predicates", "missing `fluid-replaceable`"))?;

    let height = i64::from(plan.terrain_height(x, z));
    let clearance = i64::from(clearance_voxels);
    let footing_y = height;
    let max_y = footing_y.saturating_add(clearance);
    if footing_y < i64::from(plan.config().world_floor_y)
        || max_y > i64::from(plan.config().world_ceiling_y)
    {
        return Err(unsafe_cell(
            SpawnRejectV1::InsufficientClearance,
            x,
            footing_y,
            z,
        ));
    }

    let footing = inspect_spawn_cell(plan, bindings, occupancy, x, footing_y, z)?;
    reject_cell(footing, SpawnRejectV1::Fluid, x, footing_y, z)?;
    reject_cell(footing, SpawnRejectV1::Cave, x, footing_y, z)?;
    reject_cell(footing, SpawnRejectV1::Hazard, x, footing_y, z)?;
    if !footing.is_solid() {
        return Err(unsafe_cell(SpawnRejectV1::MissingFooting, x, footing_y, z));
    }

    let mut standing_y = footing_y.saturating_add(1);
    while standing_y <= max_y {
        let cell = inspect_spawn_cell(plan, bindings, occupancy, x, standing_y, z)?;
        reject_cell(cell, SpawnRejectV1::Fluid, x, standing_y, z)?;
        reject_cell(cell, SpawnRejectV1::Cave, x, standing_y, z)?;
        reject_cell(cell, SpawnRejectV1::Hazard, x, standing_y, z)?;
        if !cell.is_clearance() {
            return Err(unsafe_cell(
                SpawnRejectV1::InsufficientClearance,
                x,
                standing_y,
                z,
            ));
        }
        standing_y = standing_y.saturating_add(1);
    }

    Ok(SpawnLocationV1 {
        footing: [x, footing_y, z],
        chunk: world_chunk(x, footing_y, z, plan.config().chunk_edge_voxels)?,
        style: plan.material_style(x, z),
    })
}

/// Selects one deterministic safe surface spawn inside `bounds`.
///
/// Candidate columns are ranked by a seed-derived column hash. Search order
/// cannot change the winner. Style comes from the D4/D7 territory query.
///
/// # Errors
///
/// Returns [`WorldgenError::NoSafeSpawn`] when no ready column passes every
/// safety check, or any arithmetic error from chunk mapping.
pub fn select_safe_spawn(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    occupancy: &SpawnOccupancyViewV1,
    bounds: SpawnSearchBoundsV1,
) -> WorldgenResult<SpawnLocationV1> {
    select_safe_spawn_with_preference(plan, bindings, occupancy, bounds, None)
}

/// Selects a deterministic safe spawn, preferring a terrain style when one is
/// available in the search bounds.
///
/// The preference is a tie-break policy above the seed-derived column hash,
/// not a content or block identity. If no safe column has `preferred_style`,
/// the selector falls back to the same deterministic ranking as
/// [`select_safe_spawn`].
///
/// # Errors
///
/// Returns [`WorldgenError::NoSafeSpawn`] when no ready column passes every
/// safety check, or any arithmetic error from chunk mapping.
pub fn select_safe_spawn_prefer_style(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    occupancy: &SpawnOccupancyViewV1,
    bounds: SpawnSearchBoundsV1,
    preferred_style: TerrainStyleV1,
) -> WorldgenResult<SpawnLocationV1> {
    select_safe_spawn_with_preference(plan, bindings, occupancy, bounds, Some(preferred_style))
}

fn select_safe_spawn_with_preference(
    plan: &GenerationPlanV1,
    bindings: &AuthoredWorldgenBindingsV1,
    occupancy: &SpawnOccupancyViewV1,
    bounds: SpawnSearchBoundsV1,
    preferred_style: Option<TerrainStyleV1>,
) -> WorldgenResult<SpawnLocationV1> {
    let mut winner: Option<(u8, u64, i64, i64, SpawnLocationV1)> = None;
    let mut z = bounds.min_z;
    while z < bounds.max_z {
        let mut x = bounds.min_x;
        while x < bounds.max_x {
            match evaluate_spawn_column(plan, bindings, occupancy, x, z, bounds.clearance_voxels) {
                Ok(location) => {
                    let rank = hash_u64(
                        SPAWN_COLUMN_DOMAIN,
                        &[
                            plan.world_seed().as_bytes(),
                            plan.generation_input_hash().as_bytes(),
                            &x.to_be_bytes(),
                            &z.to_be_bytes(),
                        ],
                    );
                    let style_penalty =
                        u8::from(preferred_style.is_some_and(|style| style != location.style()));
                    let candidate = (style_penalty, rank, x, z, location);
                    if winner.as_ref().is_none_or(|current| {
                        (&candidate.0, &candidate.1, &candidate.2, &candidate.3)
                            < (&current.0, &current.1, &current.2, &current.3)
                    }) {
                        winner = Some(candidate);
                    }
                }
                Err(
                    WorldgenError::UnreadySpawnChunk { .. } | WorldgenError::UnsafeSpawnCell { .. },
                ) => {}
                Err(error) => return Err(error),
            }
            x = x.saturating_add(1);
        }
        z = z.saturating_add(1);
    }
    winner
        .map(|(_, _, _, _, location)| location)
        .ok_or(WorldgenError::NoSafeSpawn)
}
fn reject_cell(
    cell: SpawnCellInspectionV1,
    reason: SpawnRejectV1,
    x: i64,
    y: i64,
    z: i64,
) -> WorldgenResult<()> {
    let failed = match reason {
        SpawnRejectV1::Fluid => cell.is_fluid(),
        SpawnRejectV1::Cave => cell.is_cave(),
        SpawnRejectV1::Hazard => cell.is_hazard(),
        SpawnRejectV1::MissingFooting | SpawnRejectV1::InsufficientClearance => false,
    };
    if failed {
        Err(unsafe_cell(reason, x, y, z))
    } else {
        Ok(())
    }
}

fn unsafe_cell(reason: SpawnRejectV1, x: i64, y: i64, z: i64) -> WorldgenError {
    WorldgenError::UnsafeSpawnCell {
        reason: reason.as_str(),
        x,
        y,
        z,
    }
}

fn parse_identity(field: &'static str, value: &str, kind: &str) -> WorldgenResult<StableId> {
    let identity =
        StableId::from_str(value).map_err(|error| invalid_bindings(field, error.to_string()))?;
    if identity.kind() != kind {
        return Err(invalid_bindings(
            field,
            format!("expected `{kind}` identity, got `{value}`"),
        ));
    }
    Ok(identity)
}

fn invalid_bindings(field: &'static str, reason: impl Into<String>) -> WorldgenError {
    WorldgenError::InvalidAuthoredBindings {
        field,
        reason: reason.into(),
    }
}

fn world_chunk(x: i64, y: i64, z: i64, edge: u16) -> WorldgenResult<ChunkCoordinate> {
    let edge = i64::from(edge);
    Ok(ChunkCoordinate::new(
        i32_chunk_axis(x.div_euclid(edge), "spawn chunk X")?,
        i32_chunk_axis(y.div_euclid(edge), "spawn chunk Y")?,
        i32_chunk_axis(z.div_euclid(edge), "spawn chunk Z")?,
    ))
}

fn local_voxel(x: i64, y: i64, z: i64, edge: u16) -> WorldgenResult<(u16, u16, u16)> {
    let edge = i64::from(edge);
    Ok((
        u16_local(x.rem_euclid(edge), "spawn local X")?,
        u16_local(y.rem_euclid(edge), "spawn local Y")?,
        u16_local(z.rem_euclid(edge), "spawn local Z")?,
    ))
}

fn i32_chunk_axis(value: i64, operation: &'static str) -> WorldgenResult<i32> {
    i32::try_from(value).map_err(|_| WorldgenError::ArithmeticOverflow { operation })
}

fn u16_local(value: i64, operation: &'static str) -> WorldgenResult<u16> {
    u16::try_from(value).map_err(|_| WorldgenError::ArithmeticOverflow { operation })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredBindingsDocumentV1 {
    authoring_schema: String,
    #[allow(dead_code, reason = "revision is validated by schema identity")]
    bindings_revision: u32,
    #[allow(dead_code, reason = "owner is package provenance, not a spawn input")]
    owner_package: String,
    roles: Vec<AuthoredRoleRowV1>,
    predicates: Vec<AuthoredPredicateRowV1>,
    #[allow(dead_code, reason = "provenance rows are not spawn-safety inputs")]
    provenance: Vec<AuthoredProvenanceRowV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredRoleRowV1 {
    id: String,
    candidate: String,
    cardinality: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredPredicateRowV1 {
    id: String,
    #[allow(
        dead_code,
        reason = "evaluation kind is package schema, not a spawn input"
    )]
    evaluation: String,
    #[allow(
        dead_code,
        reason = "clause lists are package schema, not a spawn input"
    )]
    clauses: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AuthoredProvenanceRowV1 {
    #[allow(dead_code, reason = "provenance identity is not a spawn-safety input")]
    id: String,
    #[allow(dead_code, reason = "source package is not a spawn-safety input")]
    source_package: String,
    #[allow(dead_code, reason = "record names are not spawn-safety inputs")]
    records: Vec<String>,
}
