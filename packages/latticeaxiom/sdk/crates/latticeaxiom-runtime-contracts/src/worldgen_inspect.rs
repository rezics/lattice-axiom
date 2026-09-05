//! Bounded worldgen and territory inspect reports.
//!
//! These contracts project already-computed generation facts into deterministic
//! diagnostic rows. They do not run terrain, cave, hydrology, or spawn
//! algorithms, allocate world-space geometry, or scan an unbounded map.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_compose::CostClass;
use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, IdentifierError, SchemaId, StableId, canonical_json_bytes,
    canonical_json_hash,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use crate::{
    DiagnosticReportInputV1, EngineEpoch, ReportRecordKeyV1, ReportSectionV1, ReportSensitivityV1,
    SubscriptionPlanner, WorldEpoch,
};

/// Inspect source for a territory ownership query.
pub const WORLDGEN_TERRITORY_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-territory@1";
/// Inspect source for a planning-cell epoch boundary receipt.
pub const WORLDGEN_BOUNDARY_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-boundary@1";
/// Inspect source for a bounded river/basin plan.
pub const WORLDGEN_RIVER_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-river@1";
/// Inspect source for a bounded geologic body.
pub const WORLDGEN_GEOLOGY_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-geology@1";
/// Inspect source for a bounded resource field.
pub const WORLDGEN_RESOURCE_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-resource@1";
/// Inspect source for a vegetation point procedure.
pub const WORLDGEN_VEGETATION_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-vegetation@1";
/// Inspect source for spawn-selection provenance.
pub const WORLDGEN_SPAWN_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-spawn@1";
/// Inspect source for a cave portal contract.
pub const WORLDGEN_PORTAL_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-cave-portal@1";
/// Inspect source for a required cave surface entrance.
pub const WORLDGEN_ENTRANCE_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-cave-entrance@1";
/// Inspect source for cave topology ownership.
pub const WORLDGEN_CAVE_OWNERSHIP_INSPECT_ID: &str =
    "latticeaxiom:inspect/worldgen-cave-ownership@1";
/// Inspect source for a rejected cave contributor.
pub const WORLDGEN_CAVE_REJECTION_INSPECT_ID: &str =
    "latticeaxiom:inspect/worldgen-cave-rejection@1";
/// Inspect source for bounded cave SDF evaluation cost.
pub const WORLDGEN_CAVE_SDF_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-cave-sdf@1";
/// Inspect source for cave connectivity and passability.
pub const WORLDGEN_CAVE_CONNECTIVITY_INSPECT_ID: &str =
    "latticeaxiom:inspect/worldgen-cave-connectivity@1";
/// Inspect source for a hydrology/fluid occupancy decision.
pub const WORLDGEN_FLUID_DECISION_INSPECT_ID: &str =
    "latticeaxiom:inspect/worldgen-fluid-decision@1";
/// Inspect source for a cross-planning-cell cave/hydrology seam.
pub const WORLDGEN_PLANNING_SEAM_INSPECT_ID: &str = "latticeaxiom:inspect/worldgen-planning-seam@1";

/// Canonical schema of a compiled worldgen inspect report.
pub const WORLDGEN_INSPECT_REPORT_SCHEMA_V1: &str = "latticeaxiom:schema/worldgen-inspect-report@1";

/// D2-aligned hard radius in planning cells for one inspect query.
pub const MAX_WORLDGEN_INSPECT_RADIUS_CELLS: u32 = 8;
/// Defensive cap on compiled report rows.
pub const DEFAULT_MAX_WORLDGEN_INSPECT_RECORDS: usize = 256;
/// Defensive cap on samples accepted for one inspect kind.
pub const DEFAULT_MAX_WORLDGEN_INSPECT_SAMPLES_PER_KIND: usize = 64;
/// Defensive cap on canonical report bytes.
pub const DEFAULT_MAX_WORLDGEN_INSPECT_BYTES: usize = 64 * 1_024;

/// Worldgen inspect channel requested by a bounded diagnostic report.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorldgenInspectKindV1 {
    /// Territory winner, runner-up, boundary distance, and provenance.
    Territory,
    /// Adjacent-epoch boundary adapter receipt.
    Boundary,
    /// Finite river/basin plan.
    River,
    /// Queryable geologic body or strata field.
    Geology,
    /// Stable resource distribution.
    Resource,
    /// Vegetation procedure with an exclusion radius.
    Vegetation,
    /// Spawn candidate provenance and reject reason.
    Spawn,
    /// Cave portal position, tangent, clearance, and fluid contract.
    Portal,
    /// Required surface entrance across planning cells and topology domains.
    Entrance,
    /// Cave topology ownership at one planning cell.
    CaveOwnership,
    /// Rejected cave contributor and machine-readable reason.
    CaveRejection,
    /// Bounded SDF evaluation cost and occupancy decision.
    CaveSdf,
    /// Graph connectivity versus voxel passability.
    CaveConnectivity,
    /// Aquifer or water/lava occupancy decision in a final cave void.
    FluidDecision,
    /// Cross-planning-cell cave face and fluid seam.
    PlanningSeam,
}

impl WorldgenInspectKindV1 {
    /// V5 natural-layer inspect kinds in canonical report order.
    pub const NATURAL: [Self; 7] = [
        Self::Territory,
        Self::Boundary,
        Self::River,
        Self::Geology,
        Self::Resource,
        Self::Vegetation,
        Self::Spawn,
    ];

    /// V6 cave and hydrology inspect kinds in canonical report order.
    pub const CAVE_HYDROLOGY: [Self; 8] = [
        Self::Portal,
        Self::Entrance,
        Self::CaveOwnership,
        Self::CaveRejection,
        Self::CaveSdf,
        Self::CaveConnectivity,
        Self::FluidDecision,
        Self::PlanningSeam,
    ];

    /// Every inspect kind in canonical report order.
    pub const ALL: [Self; 15] = [
        Self::Territory,
        Self::Boundary,
        Self::River,
        Self::Geology,
        Self::Resource,
        Self::Vegetation,
        Self::Spawn,
        Self::Portal,
        Self::Entrance,
        Self::CaveOwnership,
        Self::CaveRejection,
        Self::CaveSdf,
        Self::CaveConnectivity,
        Self::FluidDecision,
        Self::PlanningSeam,
    ];

    /// Returns the stable inspect source ID for this kind.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::InvalidIdentifier`] if the compiled-in
    /// source ID is not a canonical [`StableId`].
    pub fn source_id(self) -> Result<StableId, WorldgenInspectError> {
        parse_stable_id(self.source_id_str())
    }

    /// Returns the typed sample schema for this kind.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::InvalidIdentifier`] if the compiled-in
    /// schema ID is not a canonical [`SchemaId`].
    pub fn sample_schema(self) -> Result<SchemaId, WorldgenInspectError> {
        parse_schema_id(match self {
            Self::Territory => "latticeaxiom:schema/worldgen-territory-inspect@1",
            Self::Boundary => "latticeaxiom:schema/worldgen-boundary-inspect@1",
            Self::River => "latticeaxiom:schema/worldgen-river-inspect@1",
            Self::Geology => "latticeaxiom:schema/worldgen-geology-inspect@1",
            Self::Resource => "latticeaxiom:schema/worldgen-resource-inspect@1",
            Self::Vegetation => "latticeaxiom:schema/worldgen-vegetation-inspect@1",
            Self::Spawn => "latticeaxiom:schema/worldgen-spawn-inspect@1",
            Self::Portal => "latticeaxiom:schema/worldgen-cave-portal-inspect@1",
            Self::Entrance => "latticeaxiom:schema/worldgen-cave-entrance-inspect@1",
            Self::CaveOwnership => "latticeaxiom:schema/worldgen-cave-ownership-inspect@1",
            Self::CaveRejection => "latticeaxiom:schema/worldgen-cave-rejection-inspect@1",
            Self::CaveSdf => "latticeaxiom:schema/worldgen-cave-sdf-inspect@1",
            Self::CaveConnectivity => "latticeaxiom:schema/worldgen-cave-connectivity-inspect@1",
            Self::FluidDecision => "latticeaxiom:schema/worldgen-fluid-decision-inspect@1",
            Self::PlanningSeam => "latticeaxiom:schema/worldgen-planning-seam-inspect@1",
        })
    }

    /// Returns the collection cost. None of these sources is free by default.
    #[must_use]
    pub const fn cost(self) -> CostClass {
        match self {
            Self::Territory
            | Self::Boundary
            | Self::Portal
            | Self::Entrance
            | Self::CaveOwnership => CostClass::Moderate,
            Self::River
            | Self::Geology
            | Self::Resource
            | Self::Vegetation
            | Self::Spawn
            | Self::CaveRejection
            | Self::CaveSdf
            | Self::CaveConnectivity
            | Self::FluidDecision
            | Self::PlanningSeam => CostClass::Expensive,
        }
    }

    const fn source_id_str(self) -> &'static str {
        match self {
            Self::Territory => WORLDGEN_TERRITORY_INSPECT_ID,
            Self::Boundary => WORLDGEN_BOUNDARY_INSPECT_ID,
            Self::River => WORLDGEN_RIVER_INSPECT_ID,
            Self::Geology => WORLDGEN_GEOLOGY_INSPECT_ID,
            Self::Resource => WORLDGEN_RESOURCE_INSPECT_ID,
            Self::Vegetation => WORLDGEN_VEGETATION_INSPECT_ID,
            Self::Spawn => WORLDGEN_SPAWN_INSPECT_ID,
            Self::Portal => WORLDGEN_PORTAL_INSPECT_ID,
            Self::Entrance => WORLDGEN_ENTRANCE_INSPECT_ID,
            Self::CaveOwnership => WORLDGEN_CAVE_OWNERSHIP_INSPECT_ID,
            Self::CaveRejection => WORLDGEN_CAVE_REJECTION_INSPECT_ID,
            Self::CaveSdf => WORLDGEN_CAVE_SDF_INSPECT_ID,
            Self::CaveConnectivity => WORLDGEN_CAVE_CONNECTIVITY_INSPECT_ID,
            Self::FluidDecision => WORLDGEN_FLUID_DECISION_INSPECT_ID,
            Self::PlanningSeam => WORLDGEN_PLANNING_SEAM_INSPECT_ID,
        }
    }
}

/// Closed spawn-reject labels projected from spawn inspection.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SpawnInspectRejectV1 {
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

/// Half-open planning-cell rectangle used by plan-shaped inspect rows.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldgenInspectBoundsV1 {
    /// Inclusive minimum planning-cell x.
    pub min_x: i64,
    /// Inclusive minimum planning-cell z.
    pub min_z: i64,
    /// Exclusive maximum planning-cell x.
    pub max_x_exclusive: i64,
    /// Exclusive maximum planning-cell z.
    pub max_z_exclusive: i64,
}

impl WorldgenInspectBoundsV1 {
    /// Creates a non-empty half-open rectangle.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::InvalidBounds`] when an axis is empty or
    /// inverted.
    #[allow(
        clippy::similar_names,
        reason = "x and z half-open bounds are intentionally symmetric"
    )]
    pub const fn new(
        min_x: i64,
        min_z: i64,
        max_x_exclusive: i64,
        max_z_exclusive: i64,
    ) -> Result<Self, WorldgenInspectError> {
        if min_x >= max_x_exclusive {
            return Err(WorldgenInspectError::InvalidBounds {
                axis: "x",
                minimum: min_x,
                maximum: max_x_exclusive,
            });
        }
        if min_z >= max_z_exclusive {
            return Err(WorldgenInspectError::InvalidBounds {
                axis: "z",
                minimum: min_z,
                maximum: max_z_exclusive,
            });
        }
        Ok(Self {
            min_x,
            min_z,
            max_x_exclusive,
            max_z_exclusive,
        })
    }

    /// Returns whether a planning cell lies inside this rectangle.
    #[must_use]
    pub const fn contains_cell(self, cell_x: i64, cell_z: i64) -> bool {
        cell_x >= self.min_x
            && cell_x < self.max_x_exclusive
            && cell_z >= self.min_z
            && cell_z < self.max_z_exclusive
    }
}

/// Territory query projection matching the D7 `TerritoryQuery` result shape.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TerritoryInspectFactsV1 {
    /// Frozen biome or style identity that won this cell.
    pub winner: StableId,
    /// Ranked runner-up identity.
    pub runner_up: StableId,
    /// Distance to the nearest ownership boundary in voxels.
    pub boundary_distance_voxels: u32,
    /// Exclusive primary owner of the winning domain.
    pub primary_owner: StableId,
    /// Exclusive primary owner of the runner-up domain.
    pub secondary_owner: StableId,
    /// Named transition provider identity.
    pub transition_provider: StableId,
    /// Owner-managed transition algorithm revision.
    pub transition_revision: u32,
    /// Finite transition half-width in voxels.
    pub transition_width_voxels: u16,
    /// Whether this sample lies inside the transition band.
    pub in_transition_band: bool,
    /// Adjacent identity used by the named transition.
    pub adjacent: StableId,
    /// Deterministic transition/query provenance.
    pub provenance: CanonicalHash,
}

impl TerritoryInspectFactsV1 {
    /// Rejects a ranking that cannot be ordered.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::EqualTerritoryRanking`] when winner and
    /// runner-up are the same identity.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.winner == self.runner_up {
            Err(WorldgenInspectError::EqualTerritoryRanking {
                identity: self.winner.clone(),
            })
        } else {
            Ok(())
        }
    }
}

/// Epoch-boundary adapter receipt projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BoundaryInspectFactsV1 {
    /// Direction-independent boundary hash.
    pub boundary_hash: CanonicalHash,
    /// Frozen epoch on one side of the shared boundary.
    pub epoch_a: CanonicalHash,
    /// Frozen epoch on the other side of the shared boundary.
    pub epoch_b: CanonicalHash,
    /// Adapter identity used to join the two epochs.
    pub adapter_id: StableId,
    /// Adapter version.
    pub adapter_version: u32,
    /// Verified adapter artifact hash.
    pub adapter_hash: CanonicalHash,
    /// Finite transition width in voxels or cells.
    pub transition_width: u32,
    /// Deterministic terrain signature across the seam.
    pub terrain_boundary_signature: CanonicalHash,
    /// Required cave portal hashes in canonical order.
    pub required_cave_portals: Vec<CanonicalHash>,
}

impl BoundaryInspectFactsV1 {
    /// Validates differing epochs, a positive width, and unique sorted portals.
    ///
    /// # Errors
    ///
    /// Returns a boundary validation error for equal epochs, a zero width, a
    /// zero adapter version, or unsorted/duplicate portals.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.epoch_a == self.epoch_b {
            return Err(WorldgenInspectError::EqualBoundaryEpochs {
                epoch: self.epoch_a,
            });
        }
        if self.adapter_version == 0 {
            return Err(WorldgenInspectError::ZeroAdapterVersion {
                adapter: self.adapter_id.clone(),
            });
        }
        if self.transition_width == 0 {
            return Err(WorldgenInspectError::ZeroTransitionWidth {
                boundary: self.boundary_hash,
            });
        }
        if self
            .required_cave_portals
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(WorldgenInspectError::UnsortedPortalHashes {
                boundary: self.boundary_hash,
            });
        }
        Ok(())
    }
}

/// Bounded river/basin plan projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RiverInspectFactsV1 {
    /// Hydrology plan identity.
    pub plan_id: StableId,
    /// Basin identity contributing to the river.
    pub basin_id: StableId,
    /// Optional directed link identity.
    pub connection_id: Option<StableId>,
    /// Finite planning-cell coverage.
    pub bounds: WorldgenInspectBoundsV1,
    /// Abstract drainage rank.
    pub elevation_rank: i32,
    /// Non-zero basin capacity.
    pub capacity_units: u64,
    /// Dependency receipt for this plan fragment.
    pub dependency_receipt: CanonicalHash,
}

impl RiverInspectFactsV1 {
    /// Validates finite bounds and a non-zero capacity.
    ///
    /// # Errors
    ///
    /// Returns a bounds or capacity error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        WorldgenInspectBoundsV1::new(
            self.bounds.min_x,
            self.bounds.min_z,
            self.bounds.max_x_exclusive,
            self.bounds.max_z_exclusive,
        )?;
        if self.capacity_units == 0 {
            return Err(WorldgenInspectError::ZeroCapacity {
                id: self.basin_id.clone(),
            });
        }
        Ok(())
    }
}

/// Bounded geologic body projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeologyInspectFactsV1 {
    /// Geologic body identity.
    pub body_id: StableId,
    /// Finite planning-cell coverage.
    pub bounds: WorldgenInspectBoundsV1,
    /// Inclusive minimum world y.
    pub min_y: i64,
    /// Exclusive maximum world y.
    pub max_y_exclusive: i64,
    /// Frozen Role used to materialize the stratum.
    pub stratum_role: StableId,
    /// Frozen candidate block bound to that Role.
    pub stratum_block: StableId,
    /// Dependency receipt for this body.
    pub dependency_receipt: CanonicalHash,
}

impl GeologyInspectFactsV1 {
    /// Validates finite horizontal and vertical bounds.
    ///
    /// # Errors
    ///
    /// Returns a bounds error when any axis is empty or inverted.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        WorldgenInspectBoundsV1::new(
            self.bounds.min_x,
            self.bounds.min_z,
            self.bounds.max_x_exclusive,
            self.bounds.max_z_exclusive,
        )?;
        if self.min_y >= self.max_y_exclusive {
            return Err(WorldgenInspectError::InvalidBounds {
                axis: "y",
                minimum: self.min_y,
                maximum: self.max_y_exclusive,
            });
        }
        Ok(())
    }
}

/// Bounded resource-field projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceInspectFactsV1 {
    /// Resource field identity.
    pub field_id: StableId,
    /// Frozen placement Role.
    pub role: StableId,
    /// Frozen Predicate that accepted the placement.
    pub predicate: StableId,
    /// Concrete block bound from the Role.
    pub candidate: StableId,
    /// Predicate evaluations performed in this query.
    pub samples: u64,
    /// Accepted placements in this query.
    pub accepts: u64,
    /// Finite planning-cell coverage.
    pub bounds: WorldgenInspectBoundsV1,
    /// Dependency receipt for this field.
    pub dependency_receipt: CanonicalHash,
}

impl ResourceInspectFactsV1 {
    /// Validates bounds and `accepts <= samples`.
    ///
    /// # Errors
    ///
    /// Returns a bounds or counter error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        WorldgenInspectBoundsV1::new(
            self.bounds.min_x,
            self.bounds.min_z,
            self.bounds.max_x_exclusive,
            self.bounds.max_z_exclusive,
        )?;
        if self.accepts > self.samples {
            return Err(WorldgenInspectError::AcceptsExceedSamples {
                id: self.field_id.clone(),
                accepts: self.accepts,
                samples: self.samples,
            });
        }
        Ok(())
    }
}

/// Vegetation point-procedure projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VegetationInspectFactsV1 {
    /// Vegetation procedure identity.
    pub procedure_id: StableId,
    /// Frozen placement Role.
    pub role: StableId,
    /// Frozen Predicate that accepted the placement.
    pub predicate: StableId,
    /// Concrete block bound from the Role.
    pub candidate: StableId,
    /// Exclusive radius in voxels.
    pub exclusion_radius_voxels: u32,
    /// Finite planning-cell coverage.
    pub bounds: WorldgenInspectBoundsV1,
    /// Dependency receipt for this procedure.
    pub dependency_receipt: CanonicalHash,
}

impl VegetationInspectFactsV1 {
    /// Validates bounds and a positive exclusion radius.
    ///
    /// # Errors
    ///
    /// Returns a bounds or radius error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        WorldgenInspectBoundsV1::new(
            self.bounds.min_x,
            self.bounds.min_z,
            self.bounds.max_x_exclusive,
            self.bounds.max_z_exclusive,
        )?;
        if self.exclusion_radius_voxels == 0 {
            return Err(WorldgenInspectError::ZeroExclusionRadius {
                id: self.procedure_id.clone(),
            });
        }
        Ok(())
    }
}

/// Spawn-selection provenance projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnInspectFactsV1 {
    /// World voxel x of the inspected column.
    pub voxel_x: i64,
    /// World voxel y of the footing or reject cell.
    pub voxel_y: i64,
    /// World voxel z of the inspected column.
    pub voxel_z: i64,
    /// Territory or biome identity at the column.
    pub territory: StableId,
    /// Frozen empty/air Role used for clearance.
    pub empty_role: StableId,
    /// Frozen surface Role used for footing.
    pub surface_role: StableId,
    /// Frozen empty-placement Predicate.
    pub empty_predicate: StableId,
    /// Frozen surface-placement Predicate.
    pub surface_predicate: StableId,
    /// First reject reason, if the column is unsafe.
    pub reject: Option<SpawnInspectRejectV1>,
    /// Whether required chunks carried generation receipts.
    pub ready: bool,
    /// Generation receipt hash for the footing chunk.
    pub receipt_hash: CanonicalHash,
}

impl SpawnInspectFactsV1 {
    /// Spawn facts are always structurally valid once identities parse.
    ///
    /// # Errors
    ///
    /// This method currently cannot fail; it exists for a uniform validate API.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        Ok(())
    }
}

/// World axis used as a cave portal tangent in inspect projections.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveInspectAxisV1 {
    /// Positive or negative x alignment.
    X,
    /// Positive or negative y alignment.
    Y,
    /// Positive or negative z alignment.
    Z,
}

/// Abstract hydrology compatibility projected from a cave portal contract.
#[derive(Clone, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case", tag = "type", deny_unknown_fields)]
pub enum PortalInspectFluidV1 {
    /// No planned cross-boundary drainage.
    Dry,
    /// Topology must remain sealed to abstract hydrology.
    Sealed,
    /// Portal carries one declared abstract drainage connection.
    Drainage {
        /// Frozen hydrology-link identity.
        connection_id: StableId,
    },
}

/// Closed reasons a cave contributor was rejected during arbitration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CaveInspectRejectV1 {
    /// The contributor exceeded its declared budget.
    BudgetExceeded,
    /// The contributor would close required portal clearance.
    PortalClearance,
    /// The contributor would violate minimum surface cover.
    MinimumCover,
    /// Spatial arbitration selected another contributor.
    SpatialArbitration,
    /// An optional portal or destination was left unmet.
    OptionalUnmet,
    /// A hydrology drainage constraint could not be satisfied.
    HydrologyConstraint,
    /// The compositor identity was unknown at plan compile.
    UnknownCompositor,
    /// The contributor intersected a protected volume.
    ProtectedVolume,
    /// A documented fallback replaced the contributor.
    FallbackUsed,
}

/// Final water/lava occupancy after cave arbitration.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FluidOccupancyInspectV1 {
    /// The inspected void remains empty.
    Empty,
    /// Water occupancy was accepted.
    Water,
    /// Lava occupancy was accepted.
    Lava,
    /// The void is sealed against fluid.
    Sealed,
}

/// Cave portal contract projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PortalInspectFactsV1 {
    /// Direction-independent portal identity hash.
    pub portal_hash: CanonicalHash,
    /// First topology domain, canonically ordered with `second_domain`.
    pub first_domain: StableId,
    /// Second topology domain.
    pub second_domain: StableId,
    /// Cardinal planning-cell x offset to the opposite portal endpoint.
    pub neighbor_cell_x: i64,
    /// Cardinal planning-cell z offset to the opposite portal endpoint.
    pub neighbor_cell_z: i64,
    /// Aperture position in world millimeters.
    pub position_millimeters: [i64; 3],
    /// Tangent lying in the shared planning-cell plane.
    pub tangent_axis: CaveInspectAxisV1,
    /// Non-zero clearance width in millimeters.
    pub clearance_width_millimeters: u32,
    /// Non-zero clearance height in millimeters.
    pub clearance_height_millimeters: u32,
    /// Abstract fluid compatibility. Hydrology constrains drainage only.
    pub fluid: PortalInspectFluidV1,
    /// Dependency receipt for this portal fragment.
    pub dependency_receipt: CanonicalHash,
}

impl PortalInspectFactsV1 {
    /// Validates distinct domains, a cardinal neighbor offset, and clearance.
    ///
    /// `neighbor_cell_x`/`neighbor_cell_z` are offsets from the sample cell.
    ///
    /// # Errors
    ///
    /// Returns an ownership, adjacency, clearance, or hydrology-link error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.first_domain == self.second_domain {
            return Err(WorldgenInspectError::EqualTerritoryRanking {
                identity: self.first_domain.clone(),
            });
        }
        if !cardinal_planning_offset(self.neighbor_cell_x, self.neighbor_cell_z) {
            return Err(WorldgenInspectError::NonCardinalSeam {
                cell_x: 0,
                cell_z: 0,
                neighbor_cell_x: self.neighbor_cell_x,
                neighbor_cell_z: self.neighbor_cell_z,
            });
        }
        if self.clearance_width_millimeters == 0 || self.clearance_height_millimeters == 0 {
            return Err(WorldgenInspectError::ZeroClearance {
                id: self.first_domain.clone(),
            });
        }
        validate_hydrology_link(self.fluid.connection_id())?;
        Ok(())
    }
}

impl PortalInspectFluidV1 {
    fn connection_id(&self) -> Option<&StableId> {
        match self {
            Self::Drainage { connection_id } => Some(connection_id),
            Self::Dry | Self::Sealed => None,
        }
    }
}

/// Required cave entrance projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntranceInspectFactsV1 {
    /// Surface opening in world voxels.
    pub voxel_x: i64,
    /// Surface opening height in world voxels.
    pub voxel_y: i64,
    /// Surface opening depth in world voxels.
    pub voxel_z: i64,
    /// Ordered planning-cell count from the surface opening to the destination.
    pub cell_count: u32,
    /// Distinct topology domains visited, in first-seen order.
    pub domains: Vec<StableId>,
    /// Sorted cross-domain portal hashes used by this entrance.
    pub portals: Vec<CanonicalHash>,
    /// Underground destination topology domain.
    pub destination_domain: StableId,
    /// Destination planning-cell x.
    pub destination_cell_x: i64,
    /// Destination planning-cell z.
    pub destination_cell_z: i64,
    /// Abstract fluid state at the surface opening.
    pub fluid: PortalInspectFluidV1,
    /// Whether required chunks carried generation and collider receipts.
    pub ready: bool,
    /// Dependency receipt for this entrance.
    pub dependency_receipt: CanonicalHash,
}

impl EntranceInspectFactsV1 {
    /// Validates a four-cell, two-domain entrance with sorted unique portals.
    ///
    /// # Errors
    ///
    /// Returns a coverage, duplicate-domain, or unsorted-portal error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.cell_count < 4 {
            return Err(WorldgenInspectError::EntranceCoverage {
                cells: self.cell_count,
                domains: u32::try_from(self.domains.len()).unwrap_or(u32::MAX),
            });
        }
        if self.domains.len() < 2 {
            return Err(WorldgenInspectError::EntranceCoverage {
                cells: self.cell_count,
                domains: u32::try_from(self.domains.len()).unwrap_or(u32::MAX),
            });
        }
        let unique = self.domains.iter().collect::<BTreeSet<_>>();
        if unique.len() != self.domains.len() {
            return Err(WorldgenInspectError::DuplicateInspectList {
                kind: WorldgenInspectKindV1::Entrance,
            });
        }
        if self.portals.is_empty() || self.portals.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(WorldgenInspectError::UnsortedPortalHashes {
                boundary: self.dependency_receipt,
            });
        }
        if !self.domains.contains(&self.destination_domain) {
            return Err(WorldgenInspectError::DestinationDomainMissing {
                domain: self.destination_domain.clone(),
            });
        }
        Ok(())
    }
}

/// Cave topology ownership projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveOwnershipInspectFactsV1 {
    /// Winning cave topology domain.
    pub winner: StableId,
    /// Ranked runner-up cave topology domain.
    pub runner_up: StableId,
    /// Exclusive primary owner of the winning domain.
    pub primary_owner: StableId,
    /// Exclusive primary owner of the runner-up domain.
    pub secondary_owner: StableId,
    /// Exclusive channel, always cave topology.
    pub channel: StableId,
    /// Distance to the nearest topology ownership boundary in voxels.
    pub boundary_distance_voxels: u32,
    /// Whether this sample lies in a child-owned core.
    pub in_core: bool,
    /// Deterministic ownership provenance.
    pub provenance: CanonicalHash,
}

impl CaveOwnershipInspectFactsV1 {
    /// Rejects a ranking that cannot be ordered.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::EqualTerritoryRanking`] when winner and
    /// runner-up are the same identity.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.winner == self.runner_up {
            Err(WorldgenInspectError::EqualTerritoryRanking {
                identity: self.winner.clone(),
            })
        } else {
            Ok(())
        }
    }
}

/// Rejected cave contributor projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveRejectionInspectFactsV1 {
    /// Contributor identity that failed arbitration.
    pub contributor: StableId,
    /// Closed reject reason.
    pub reason: CaveInspectRejectV1,
    /// Budget consumed by the rejected attempt.
    pub budget_consumed: u64,
    /// Hard budget limit that applied.
    pub budget_limit: u64,
    /// Stable arbitration sort key.
    pub sort_key: CanonicalHash,
}

impl CaveRejectionInspectFactsV1 {
    /// Validates a positive budget limit and `consumed <= limit`.
    ///
    /// # Errors
    ///
    /// Returns a budget error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.budget_limit == 0 {
            return Err(WorldgenInspectError::ZeroBudget {
                id: self.contributor.clone(),
            });
        }
        if self.budget_consumed > self.budget_limit {
            return Err(WorldgenInspectError::BudgetConsumedExceedsLimit {
                id: self.contributor.clone(),
                consumed: self.budget_consumed,
                limit: self.budget_limit,
            });
        }
        Ok(())
    }
}

/// Bounded cave SDF evaluation-cost projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaveSdfInspectFactsV1 {
    /// World voxel x of the sampled occupancy.
    pub voxel_x: i64,
    /// World voxel y of the sampled occupancy.
    pub voxel_y: i64,
    /// World voxel z of the sampled occupancy.
    pub voxel_z: i64,
    /// Bounded SDF evaluations performed for this query.
    pub evaluations: u64,
    /// Local cave-cell signed distance.
    pub local_signed_distance: i32,
    /// Bounded branch-contributor signed distance.
    pub branch_signed_distance: i32,
    /// Tightest requested portal signed distance.
    pub portal_signed_distance: i32,
    /// Unioned raw cave field; non-positive samples are void.
    pub raw_signed_distance: i32,
    /// Whether final occupancy is a cave void after cover and floor.
    pub finally_void: bool,
    /// Bounded generation cost in microseconds recorded for this sample.
    pub generation_time_micros: u64,
    /// Bounded retained diagnostic bytes for this sample.
    pub memory_bytes: u64,
    /// Bounded branch contributor identity.
    pub branch_contributor: StableId,
}

impl CaveSdfInspectFactsV1 {
    /// Validates a positive evaluation count and memory bound.
    ///
    /// # Errors
    ///
    /// Returns a zero-cost error when evaluations or memory are zero.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.evaluations == 0 || self.memory_bytes == 0 {
            return Err(WorldgenInspectError::ZeroSdfCost {
                id: self.branch_contributor.clone(),
            });
        }
        Ok(())
    }
}

/// Cave graph connectivity and voxel passability projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "graph reachability, voxel passability, must-connect, and clearance are independent dual-validation flags"
)]
pub struct CaveConnectivityInspectFactsV1 {
    /// Whether the planned graph reaches the must-connect destination.
    pub graph_reachable: bool,
    /// Whether the realized occupancy is passable along the same path.
    pub passable: bool,
    /// Count of loops in the bounded graph sample.
    pub loop_count: u64,
    /// Count of dead ends in the bounded graph sample.
    pub dead_end_count: u64,
    /// Whether every must-connect destination is satisfied.
    pub must_connect_satisfied: bool,
    /// Whether required portal clearance remains intact.
    pub clearance_intact: bool,
    /// Graph samples inspected.
    pub graph_samples: u64,
    /// Passable occupancy samples among `graph_samples`.
    pub passable_samples: u64,
    /// Distinct connected topology domains, sorted.
    pub connected_domains: Vec<StableId>,
}

impl CaveConnectivityInspectFactsV1 {
    /// Validates counters and a sorted unique domain list of at least two.
    ///
    /// # Errors
    ///
    /// Returns a counter, coverage, or sorting error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.connected_domains.len() < 2
            || self
                .connected_domains
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(WorldgenInspectError::UnsortedInspectList {
                kind: WorldgenInspectKindV1::CaveConnectivity,
            });
        }
        if self.passable_samples > self.graph_samples {
            return Err(WorldgenInspectError::AcceptsExceedSamples {
                id: self.connected_domains[0].clone(),
                accepts: self.passable_samples,
                samples: self.graph_samples,
            });
        }
        Ok(())
    }
}

/// Hydrology/fluid occupancy decision projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FluidDecisionInspectFactsV1 {
    /// Final occupancy after cave arbitration.
    pub occupancy: FluidOccupancyInspectV1,
    /// Frozen Role used for the accepted fluid or empty cell.
    pub role: StableId,
    /// Frozen Predicate that accepted the occupancy.
    pub predicate: StableId,
    /// Concrete water, lava, or empty candidate.
    pub candidate: StableId,
    /// Whether this cell is an aquifer decision.
    pub aquifer: bool,
    /// Optional drainage connection that constrained the decision.
    pub drainage_connection: Option<StableId>,
    /// Whether the cave occupancy is a final void that may hold fluid.
    pub cave_finally_void: bool,
    /// Whether occupancy is continuous across the adjacent planning cell.
    pub continuous_across_seam: bool,
}

impl FluidDecisionInspectFactsV1 {
    /// Validates drainage-link identity when a connection is present.
    ///
    /// # Errors
    ///
    /// Returns an identifier error for a non hydrology-link connection.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        validate_hydrology_link(self.drainage_connection.as_ref())?;
        Ok(())
    }
}

/// Cross-planning-cell cave and hydrology seam projection.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningSeamInspectFactsV1 {
    /// Cardinal planning-cell x offset to the adjacent cell.
    pub neighbor_cell_x: i64,
    /// Cardinal planning-cell z offset to the adjacent cell.
    pub neighbor_cell_z: i64,
    /// Whether shared-face occupancy matches from both cells.
    pub shared_face_match: bool,
    /// Required cave portal hashes in canonical order.
    pub required_cave_portals: Vec<CanonicalHash>,
    /// Whether a fluid occupancy discontinuity remains unexplained.
    pub fluid_discontinuity: bool,
    /// Deterministic terrain/cave signature across the seam.
    pub seam_signature: CanonicalHash,
}

impl PlanningSeamInspectFactsV1 {
    /// Validates a cardinal neighbor and unique sorted portal hashes.
    ///
    /// # Errors
    ///
    /// Returns a seam or portal-order error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if !cardinal_planning_offset(self.neighbor_cell_x, self.neighbor_cell_z) {
            return Err(WorldgenInspectError::NonCardinalSeam {
                cell_x: 0,
                cell_z: 0,
                neighbor_cell_x: self.neighbor_cell_x,
                neighbor_cell_z: self.neighbor_cell_z,
            });
        }
        if self
            .required_cave_portals
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(WorldgenInspectError::UnsortedPortalHashes {
                boundary: self.seam_signature,
            });
        }
        Ok(())
    }
}

fn cardinal_planning_offset(neighbor_cell_x: i64, neighbor_cell_z: i64) -> bool {
    matches!(
        (neighbor_cell_x, neighbor_cell_z),
        (0, 1 | -1) | (1 | -1, 0)
    )
}

fn validate_hydrology_link(connection_id: Option<&StableId>) -> Result<(), WorldgenInspectError> {
    match connection_id {
        Some(connection_id) if connection_id.kind() != "hydrology-link" => {
            Err(WorldgenInspectError::InvalidStableKind {
                value: connection_id.clone(),
                expected: "hydrology-link",
            })
        }
        Some(_) | None => Ok(()),
    }
}

/// Typed inspect body tagged independently of the record kind for serde.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "type")]
pub enum WorldgenInspectBodyV1 {
    /// Territory ownership sample.
    Territory(TerritoryInspectFactsV1),
    /// Boundary adapter sample.
    Boundary(BoundaryInspectFactsV1),
    /// River/basin sample.
    River(RiverInspectFactsV1),
    /// Geology sample.
    Geology(GeologyInspectFactsV1),
    /// Resource-field sample.
    Resource(ResourceInspectFactsV1),
    /// Vegetation-procedure sample.
    Vegetation(VegetationInspectFactsV1),
    /// Spawn-column sample.
    Spawn(SpawnInspectFactsV1),
    /// Cave portal sample.
    Portal(PortalInspectFactsV1),
    /// Cave entrance sample.
    Entrance(EntranceInspectFactsV1),
    /// Cave topology ownership sample.
    CaveOwnership(CaveOwnershipInspectFactsV1),
    /// Rejected cave contributor sample.
    CaveRejection(CaveRejectionInspectFactsV1),
    /// Cave SDF cost sample.
    CaveSdf(CaveSdfInspectFactsV1),
    /// Cave connectivity and passability sample.
    CaveConnectivity(CaveConnectivityInspectFactsV1),
    /// Fluid occupancy decision sample.
    FluidDecision(FluidDecisionInspectFactsV1),
    /// Cross-planning-cell seam sample.
    PlanningSeam(PlanningSeamInspectFactsV1),
}

impl WorldgenInspectBodyV1 {
    /// Returns the kind implied by this body.
    #[must_use]
    pub const fn kind(&self) -> WorldgenInspectKindV1 {
        match self {
            Self::Territory(_) => WorldgenInspectKindV1::Territory,
            Self::Boundary(_) => WorldgenInspectKindV1::Boundary,
            Self::River(_) => WorldgenInspectKindV1::River,
            Self::Geology(_) => WorldgenInspectKindV1::Geology,
            Self::Resource(_) => WorldgenInspectKindV1::Resource,
            Self::Vegetation(_) => WorldgenInspectKindV1::Vegetation,
            Self::Spawn(_) => WorldgenInspectKindV1::Spawn,
            Self::Portal(_) => WorldgenInspectKindV1::Portal,
            Self::Entrance(_) => WorldgenInspectKindV1::Entrance,
            Self::CaveOwnership(_) => WorldgenInspectKindV1::CaveOwnership,
            Self::CaveRejection(_) => WorldgenInspectKindV1::CaveRejection,
            Self::CaveSdf(_) => WorldgenInspectKindV1::CaveSdf,
            Self::CaveConnectivity(_) => WorldgenInspectKindV1::CaveConnectivity,
            Self::FluidDecision(_) => WorldgenInspectKindV1::FluidDecision,
            Self::PlanningSeam(_) => WorldgenInspectKindV1::PlanningSeam,
        }
    }

    /// Validates kind-specific invariants.
    ///
    /// # Errors
    ///
    /// Returns the inner facts validation error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        match self {
            Self::Territory(facts) => facts.validate(),
            Self::Boundary(facts) => facts.validate(),
            Self::River(facts) => facts.validate(),
            Self::Geology(facts) => facts.validate(),
            Self::Resource(facts) => facts.validate(),
            Self::Vegetation(facts) => facts.validate(),
            Self::Spawn(facts) => facts.validate(),
            Self::Portal(facts) => facts.validate(),
            Self::Entrance(facts) => facts.validate(),
            Self::CaveOwnership(facts) => facts.validate(),
            Self::CaveRejection(facts) => facts.validate(),
            Self::CaveSdf(facts) => facts.validate(),
            Self::CaveConnectivity(facts) => facts.validate(),
            Self::FluidDecision(facts) => facts.validate(),
            Self::PlanningSeam(facts) => facts.validate(),
        }
    }
}

/// One bounded inspect row. Identity is `(kind, id)` and is independent of
/// `HashMap` insertion order.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldgenInspectRecordV1 {
    /// Inspect channel.
    pub kind: WorldgenInspectKindV1,
    /// Stable row identity within that channel.
    pub id: StableId,
    /// Planning-cell x of this sample.
    pub cell_x: i64,
    /// Planning-cell z of this sample.
    pub cell_z: i64,
    /// Channel-specific facts.
    pub body: WorldgenInspectBodyV1,
}

impl WorldgenInspectRecordV1 {
    /// Creates a record after checking kind/body agreement and inner facts.
    ///
    /// # Errors
    ///
    /// Returns a kind mismatch or facts validation error.
    pub fn new(
        kind: WorldgenInspectKindV1,
        id: StableId,
        cell_x: i64,
        cell_z: i64,
        body: WorldgenInspectBodyV1,
    ) -> Result<Self, WorldgenInspectError> {
        let record = Self {
            kind,
            id,
            cell_x,
            cell_z,
            body,
        };
        record.validate()?;
        Ok(record)
    }

    fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.kind != self.body.kind() {
            return Err(WorldgenInspectError::KindBodyMismatch {
                kind: self.kind,
                body: self.body.kind(),
            });
        }
        self.body.validate()
    }

    fn sort_key(&self) -> (WorldgenInspectKindV1, &StableId) {
        (self.kind, &self.id)
    }
}

/// Explicit on-demand query covering a finite planning-cell window.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldgenInspectQueryV1 {
    /// Engine instance epoch copied into the report.
    pub engine_epoch: EngineEpoch,
    /// Active world epoch when the query depends on a world.
    pub world_epoch: Option<WorldEpoch>,
    /// Output-affecting generation input hash.
    pub generation_input_hash: CanonicalHash,
    /// Package/artifact-aware generation provenance hash.
    pub generation_provenance_hash: CanonicalHash,
    /// Planning-cell origin x; may be negative.
    pub origin_cell_x: i64,
    /// Planning-cell origin z; may be negative.
    pub origin_cell_z: i64,
    /// Inclusive Chebyshev radius in planning cells.
    pub radius_cells: u32,
    /// Requested inspect channels in canonical order.
    pub requested: BTreeSet<WorldgenInspectKindV1>,
}

impl WorldgenInspectQueryV1 {
    /// Validates radius, requested set, and coordinate window arithmetic.
    ///
    /// # Errors
    ///
    /// Returns an empty-request, radius, or overflow error.
    pub fn validate(&self) -> Result<(), WorldgenInspectError> {
        if self.requested.is_empty() {
            return Err(WorldgenInspectError::EmptyRequest);
        }
        if self.radius_cells == 0 || self.radius_cells > MAX_WORLDGEN_INSPECT_RADIUS_CELLS {
            return Err(WorldgenInspectError::RadiusOutOfRange {
                observed: self.radius_cells,
                maximum: MAX_WORLDGEN_INSPECT_RADIUS_CELLS,
            });
        }
        let _ = self.window()?;
        Ok(())
    }

    /// Returns the inclusive planning-cell window covered by this query.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::CoordinateOverflow`] when origin ± radius
    /// cannot be represented as `i64`.
    #[allow(
        clippy::similar_names,
        reason = "x and z inclusive/exclusive window edges are intentionally symmetric"
    )]
    pub fn window(&self) -> Result<WorldgenInspectBoundsV1, WorldgenInspectError> {
        let radius = i64::from(self.radius_cells);
        let min_x = self
            .origin_cell_x
            .checked_sub(radius)
            .ok_or(WorldgenInspectError::CoordinateOverflow)?;
        let max_x_inclusive = self
            .origin_cell_x
            .checked_add(radius)
            .ok_or(WorldgenInspectError::CoordinateOverflow)?;
        let min_z = self
            .origin_cell_z
            .checked_sub(radius)
            .ok_or(WorldgenInspectError::CoordinateOverflow)?;
        let max_z_inclusive = self
            .origin_cell_z
            .checked_add(radius)
            .ok_or(WorldgenInspectError::CoordinateOverflow)?;
        let max_x_exclusive = max_x_inclusive
            .checked_add(1)
            .ok_or(WorldgenInspectError::CoordinateOverflow)?;
        let max_z_exclusive = max_z_inclusive
            .checked_add(1)
            .ok_or(WorldgenInspectError::CoordinateOverflow)?;
        WorldgenInspectBoundsV1::new(min_x, min_z, max_x_exclusive, max_z_exclusive)
    }

    /// Returns whether a planning cell lies in the query window.
    ///
    /// # Errors
    ///
    /// Returns the window arithmetic error.
    pub fn contains_cell(&self, cell_x: i64, cell_z: i64) -> Result<bool, WorldgenInspectError> {
        Ok(self.window()?.contains_cell(cell_x, cell_z))
    }
}

/// Subscription snapshot that authorizes collecting inspect kinds.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldgenInspectCollectionV1 {
    subscribed: BTreeSet<WorldgenInspectKindV1>,
}

impl WorldgenInspectCollectionV1 {
    /// Creates a collection from an explicit subscribed set.
    #[must_use]
    pub fn new(subscribed: BTreeSet<WorldgenInspectKindV1>) -> Self {
        Self { subscribed }
    }

    /// Derives subscribed kinds from active planner targets.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError::InvalidIdentifier`] if a compiled-in
    /// source ID cannot be parsed.
    pub fn from_planner(planner: &SubscriptionPlanner) -> Result<Self, WorldgenInspectError> {
        let mut subscribed = BTreeSet::new();
        for kind in WorldgenInspectKindV1::ALL {
            let source = kind.source_id()?;
            if planner
                .active_targets()
                .iter()
                .any(|target| target.source == source)
            {
                subscribed.insert(kind);
            }
        }
        Ok(Self { subscribed })
    }

    /// Returns whether dedicated collection may run for `kind`.
    #[must_use]
    pub fn allows(&self, kind: WorldgenInspectKindV1) -> bool {
        self.subscribed.contains(&kind)
    }

    /// Returns subscribed kinds in canonical order.
    #[must_use]
    pub const fn subscribed(&self) -> &BTreeSet<WorldgenInspectKindV1> {
        &self.subscribed
    }
}

/// Candidate inspect rows supplied to the report compiler.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldgenInspectSamplesV1 {
    records: Vec<WorldgenInspectRecordV1>,
}

impl WorldgenInspectSamplesV1 {
    /// Accepts records in any iterator order and stores them canonically.
    ///
    /// # Errors
    ///
    /// Returns a duplicate, kind/body, or facts validation error. `HashMap`
    /// iteration order cannot change the stored sequence.
    pub fn new(
        records: impl IntoIterator<Item = WorldgenInspectRecordV1>,
    ) -> Result<Self, WorldgenInspectError> {
        let mut ordered = BTreeMap::new();
        for record in records {
            record.validate()?;
            let key = (record.kind, record.id.clone());
            if ordered.insert(key.clone(), record).is_some() {
                return Err(WorldgenInspectError::DuplicateRecord {
                    kind: key.0,
                    id: key.1,
                });
            }
        }
        Ok(Self {
            records: ordered.into_values().collect(),
        })
    }

    /// Returns records in canonical `(kind, id)` order.
    #[must_use]
    pub fn records(&self) -> &[WorldgenInspectRecordV1] {
        &self.records
    }
}

/// Defensive input and output bounds for worldgen inspect compilation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(
    clippy::struct_field_names,
    reason = "the host limit fields share a max_ prefix with the applied-limit DTO"
)]
pub struct WorldgenInspectLimits {
    max_records: usize,
    max_bytes: usize,
    max_samples_per_kind: usize,
}

impl WorldgenInspectLimits {
    /// Creates positive inspect bounds.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectLimitError`] if any bound is zero.
    pub const fn new(
        max_records: usize,
        max_bytes: usize,
        max_samples_per_kind: usize,
    ) -> Result<Self, WorldgenInspectLimitError> {
        if max_records == 0 {
            Err(WorldgenInspectLimitError::ZeroRecords)
        } else if max_bytes == 0 {
            Err(WorldgenInspectLimitError::ZeroBytes)
        } else if max_samples_per_kind == 0 {
            Err(WorldgenInspectLimitError::ZeroSamplesPerKind)
        } else {
            Ok(Self {
                max_records,
                max_bytes,
                max_samples_per_kind,
            })
        }
    }
}

impl Default for WorldgenInspectLimits {
    fn default() -> Self {
        Self {
            max_records: DEFAULT_MAX_WORLDGEN_INSPECT_RECORDS,
            max_bytes: DEFAULT_MAX_WORLDGEN_INSPECT_BYTES,
            max_samples_per_kind: DEFAULT_MAX_WORLDGEN_INSPECT_SAMPLES_PER_KIND,
        }
    }
}

/// Invalid worldgen inspect limit.
#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum WorldgenInspectLimitError {
    /// Record cap was zero.
    #[error("worldgen inspect record limit must be positive")]
    ZeroRecords,
    /// Byte cap was zero.
    #[error("worldgen inspect byte limit must be positive")]
    ZeroBytes,
    /// Per-kind sample cap was zero.
    #[error("worldgen inspect per-kind sample limit must be positive")]
    ZeroSamplesPerKind,
}

/// Applied inspect bounds retained as truncation evidence.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
#[allow(
    clippy::struct_field_names,
    reason = "applied limit fields share a max_ prefix with WorldgenInspectLimits"
)]
pub struct AppliedWorldgenInspectLimitsV1 {
    /// Maximum output records.
    pub max_records: usize,
    /// Maximum canonical report bytes.
    pub max_bytes: usize,
    /// Maximum samples retained per kind before compile.
    pub max_samples_per_kind: usize,
}

/// Versioned, bounded, canonically ordered worldgen inspect report.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldgenInspectReportV1 {
    /// Report schema identity.
    pub schema: SchemaId,
    /// Report schema version, currently one.
    pub schema_version: u32,
    /// Query that produced this report.
    pub query: WorldgenInspectQueryV1,
    /// Canonically ordered rows.
    pub records: Vec<WorldgenInspectRecordV1>,
    /// Requested kinds skipped because collection was not subscribed.
    pub skipped_kinds: BTreeSet<WorldgenInspectKindV1>,
    /// Candidate count omitted by stable suffix truncation.
    pub truncated_records: usize,
    /// Limits that caused any truncation.
    pub limits: AppliedWorldgenInspectLimitsV1,
    /// Whether any requested kind had dedicated collection enabled.
    pub dedicated_work_enabled: bool,
}

impl WorldgenInspectReportV1 {
    /// Returns canonical compact JSON.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CanonicalJsonError> {
        canonical_json_bytes(self)
    }

    /// Returns the hash of canonical report bytes.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if serialization fails.
    pub fn canonical_hash(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(self)
    }

    /// Projects this report into the generic diagnostic report builder.
    ///
    /// Generation hashes are public fingerprints. Spatial rows are coordinate
    /// sensitive and require explicit consent before disclosure.
    ///
    /// # Errors
    ///
    /// Returns [`WorldgenInspectError`] if a diagnostic key cannot be parsed or
    /// a row cannot be encoded.
    pub fn diagnostic_inputs(&self) -> Result<Vec<DiagnosticReportInputV1>, WorldgenInspectError> {
        let mut inputs = vec![
            fingerprint_input(
                "latticeaxiom:inspect/worldgen-input-hash@1",
                serde_json::to_value(self.query.generation_input_hash)
                    .map_err(WorldgenInspectError::from_json)?,
            )?,
            fingerprint_input(
                "latticeaxiom:inspect/worldgen-provenance-hash@1",
                serde_json::to_value(self.query.generation_provenance_hash)
                    .map_err(WorldgenInspectError::from_json)?,
            )?,
            budget_input(
                "latticeaxiom:inspect/worldgen-skipped-kinds@1",
                serde_json::to_value(&self.skipped_kinds)
                    .map_err(WorldgenInspectError::from_json)?,
            )?,
            budget_input(
                "latticeaxiom:inspect/worldgen-truncated-records@1",
                Value::from(self.truncated_records),
            )?,
        ];
        for record in &self.records {
            let value = serde_json::to_value(record).map_err(WorldgenInspectError::from_json)?;
            inputs.push(DiagnosticReportInputV1 {
                key: ReportRecordKeyV1 {
                    section: ReportSectionV1::World,
                    id: record.id.clone(),
                },
                value,
                sensitivity: ReportSensitivityV1::Coordinate,
            });
        }
        Ok(inputs)
    }
}

/// Compiles a bounded worldgen inspect report without changing world state.
///
/// Unsubscribed requested kinds are omitted and recorded as skipped. Samples
/// for a kind that was not subscribed are rejected so collection cannot run as
/// a side effect of report assembly.
///
/// # Errors
///
/// Returns [`WorldgenInspectError`] for query, sample, subscription, window,
/// budget, or encoding violations.
#[allow(
    clippy::too_many_lines,
    reason = "the fail-closed subscription, window, and truncation sequence stays auditable"
)]
pub fn compile_worldgen_inspect_report(
    query: &WorldgenInspectQueryV1,
    samples: &WorldgenInspectSamplesV1,
    collection: &WorldgenInspectCollectionV1,
    limits: WorldgenInspectLimits,
) -> Result<WorldgenInspectReportV1, WorldgenInspectError> {
    query.validate()?;
    let window = query.window()?;
    let mut skipped_kinds = BTreeSet::new();
    for kind in &query.requested {
        if !collection.allows(*kind) {
            skipped_kinds.insert(*kind);
        }
    }
    let mut per_kind = BTreeMap::<WorldgenInspectKindV1, usize>::new();
    let mut selected = Vec::new();
    for record in samples.records() {
        record.validate()?;
        if !query.requested.contains(&record.kind) {
            return Err(WorldgenInspectError::UnrequestedKind { kind: record.kind });
        }
        if !collection.allows(record.kind) {
            return Err(WorldgenInspectError::CollectionWithoutSubscription { kind: record.kind });
        }
        if !window.contains_cell(record.cell_x, record.cell_z) {
            return Err(WorldgenInspectError::SampleOutsideQuery {
                kind: record.kind,
                id: record.id.clone(),
                cell_x: record.cell_x,
                cell_z: record.cell_z,
            });
        }
        let count = per_kind.entry(record.kind).or_insert(0);
        *count = count
            .checked_add(1)
            .ok_or(WorldgenInspectError::SampleCountOverflow { kind: record.kind })?;
        if *count > limits.max_samples_per_kind {
            return Err(WorldgenInspectError::SamplesPerKindExceeded {
                kind: record.kind,
                observed: *count,
                maximum: limits.max_samples_per_kind,
            });
        }
        selected.push(record.clone());
    }
    selected.sort_by(|left, right| left.sort_key().cmp(&right.sort_key()));
    if let Some(pair) = selected
        .windows(2)
        .find(|pair| pair[0].sort_key() == pair[1].sort_key())
    {
        return Err(WorldgenInspectError::DuplicateRecord {
            kind: pair[0].kind,
            id: pair[0].id.clone(),
        });
    }

    let schema = parse_schema_id(WORLDGEN_INSPECT_REPORT_SCHEMA_V1)?;
    let applied = AppliedWorldgenInspectLimitsV1 {
        max_records: limits.max_records,
        max_bytes: limits.max_bytes,
        max_samples_per_kind: limits.max_samples_per_kind,
    };
    let total = selected.len();
    let count_limited = total.min(limits.max_records);
    let mut low = 0_usize;
    let mut high = count_limited;
    while low < high {
        let middle = low + (high - low).div_ceil(2);
        let candidate = WorldgenInspectReportV1 {
            schema: schema.clone(),
            schema_version: 1,
            query: query.clone(),
            records: selected[..middle].to_vec(),
            skipped_kinds: skipped_kinds.clone(),
            truncated_records: total - middle,
            limits: applied,
            dedicated_work_enabled: dedicated_work_enabled(query, collection),
        };
        let bytes = canonical_json_bytes(&candidate)?;
        if bytes.len() <= limits.max_bytes {
            low = middle;
        } else {
            high = middle.saturating_sub(1);
        }
    }
    let report = WorldgenInspectReportV1 {
        schema,
        schema_version: 1,
        query: query.clone(),
        records: selected[..low].to_vec(),
        skipped_kinds,
        truncated_records: total - low,
        limits: applied,
        dedicated_work_enabled: dedicated_work_enabled(query, collection),
    };
    let bytes = canonical_json_bytes(&report)?;
    if bytes.len() > limits.max_bytes {
        return Err(WorldgenInspectError::EnvelopeExceedsLimit {
            observed: bytes.len(),
            maximum: limits.max_bytes,
        });
    }
    Ok(report)
}

fn dedicated_work_enabled(
    query: &WorldgenInspectQueryV1,
    collection: &WorldgenInspectCollectionV1,
) -> bool {
    query.requested.iter().any(|kind| collection.allows(*kind))
}

fn fingerprint_input(
    id: &str,
    value: Value,
) -> Result<DiagnosticReportInputV1, WorldgenInspectError> {
    Ok(DiagnosticReportInputV1 {
        key: ReportRecordKeyV1 {
            section: ReportSectionV1::Fingerprint,
            id: parse_stable_id(id)?,
        },
        value,
        sensitivity: ReportSensitivityV1::Public,
    })
}

fn budget_input(id: &str, value: Value) -> Result<DiagnosticReportInputV1, WorldgenInspectError> {
    Ok(DiagnosticReportInputV1 {
        key: ReportRecordKeyV1 {
            section: ReportSectionV1::Budget,
            id: parse_stable_id(id)?,
        },
        value,
        sensitivity: ReportSensitivityV1::Public,
    })
}

fn parse_stable_id(value: &str) -> Result<StableId, WorldgenInspectError> {
    value
        .parse()
        .map_err(|source| WorldgenInspectError::InvalidIdentifier {
            value: value.to_owned(),
            source,
        })
}

fn parse_schema_id(value: &str) -> Result<SchemaId, WorldgenInspectError> {
    value
        .parse()
        .map_err(|source| WorldgenInspectError::InvalidIdentifier {
            value: value.to_owned(),
            source,
        })
}

impl WorldgenInspectError {
    fn from_json(source: serde_json::Error) -> Self {
        Self::Json { source }
    }
}

/// Invalid worldgen inspect query, sample, or report.
#[derive(Debug, Error)]
pub enum WorldgenInspectError {
    /// A compiled-in or caller identity failed canonical parsing.
    #[error("invalid worldgen inspect identifier `{value}`: {source}")]
    InvalidIdentifier {
        /// Rejected text.
        value: String,
        /// Identifier grammar error.
        #[source]
        source: IdentifierError,
    },
    /// No inspect kind was requested.
    #[error("worldgen inspect query requests no kinds")]
    EmptyRequest,
    /// Query radius was zero or above the hard cell cap.
    #[error("worldgen inspect radius {observed} is outside 1..={maximum}")]
    RadiusOutOfRange {
        /// Requested radius.
        observed: u32,
        /// Hard maximum.
        maximum: u32,
    },
    /// Origin ± radius overflowed signed 64-bit cell coordinates.
    #[error("worldgen inspect query window overflowed i64")]
    CoordinateOverflow,
    /// A half-open bounds axis was empty or inverted.
    #[error("worldgen inspect {axis} bounds [{minimum}, {maximum}) are empty")]
    InvalidBounds {
        /// Axis name.
        axis: &'static str,
        /// Inclusive minimum.
        minimum: i64,
        /// Exclusive maximum.
        maximum: i64,
    },
    /// Territory winner and runner-up were the same identity.
    #[error("territory inspect ranking repeats `{identity}`")]
    EqualTerritoryRanking {
        /// Repeated identity.
        identity: StableId,
    },
    /// Boundary receipt joined an epoch to itself.
    #[error("boundary inspect epochs are equal (`{epoch}`)")]
    EqualBoundaryEpochs {
        /// Repeated epoch hash.
        epoch: CanonicalHash,
    },
    /// Adapter version was zero.
    #[error("boundary adapter `{adapter}` version must be positive")]
    ZeroAdapterVersion {
        /// Adapter identity.
        adapter: StableId,
    },
    /// Transition width was zero.
    #[error("boundary `{boundary}` transition width must be positive")]
    ZeroTransitionWidth {
        /// Boundary hash.
        boundary: CanonicalHash,
    },
    /// Portal hashes were unsorted or duplicated.
    #[error("boundary `{boundary}` cave portals are unsorted or duplicated")]
    UnsortedPortalHashes {
        /// Boundary hash.
        boundary: CanonicalHash,
    },
    /// A stable identity used the wrong registration kind.
    #[error("inspect identity `{value}` must use kind `{expected}`")]
    InvalidStableKind {
        /// Rejected identity.
        value: StableId,
        /// Expected registration kind.
        expected: &'static str,
    },
    /// Portal or entrance clearance was zero.
    #[error("inspect row `{id}` clearance must be positive")]
    ZeroClearance {
        /// Row identity.
        id: StableId,
    },
    /// A cave entrance did not cover four cells and two topology domains.
    #[error("cave entrance covers {cells} cells and {domains} domains")]
    EntranceCoverage {
        /// Observed planning-cell count.
        cells: u32,
        /// Observed distinct domain count.
        domains: u32,
    },
    /// A list of inspect identities contained a duplicate.
    #[error("inspect `{kind:?}` list contains a duplicate identity")]
    DuplicateInspectList {
        /// Kind whose list was duplicated.
        kind: WorldgenInspectKindV1,
    },
    /// A destination domain was missing from the visited domain list.
    #[error("cave destination domain `{domain}` is not on the entrance path")]
    DestinationDomainMissing {
        /// Missing destination identity.
        domain: StableId,
    },
    /// Contributor budget limit was zero.
    #[error("inspect row `{id}` budget limit must be positive")]
    ZeroBudget {
        /// Contributor identity.
        id: StableId,
    },
    /// Consumed budget exceeded the hard limit.
    #[error("inspect row `{id}` consumed {consumed} of budget {limit}")]
    BudgetConsumedExceedsLimit {
        /// Contributor identity.
        id: StableId,
        /// Consumed units.
        consumed: u64,
        /// Hard limit.
        limit: u64,
    },
    /// SDF evaluations or retained diagnostic bytes were zero.
    #[error("inspect row `{id}` SDF cost must be positive")]
    ZeroSdfCost {
        /// Contributor identity.
        id: StableId,
    },
    /// A list of inspect identities was too short or not strictly sorted.
    #[error("inspect `{kind:?}` list is unsorted, duplicated, or too short")]
    UnsortedInspectList {
        /// Kind whose list was invalid.
        kind: WorldgenInspectKindV1,
    },
    /// A seam or portal neighbor was not a cardinal planning-cell offset.
    #[error(
        "inspect seam ({cell_x}, {cell_z}) -> ({neighbor_cell_x}, {neighbor_cell_z}) is not cardinal"
    )]
    NonCardinalSeam {
        /// Sample cell x, or zero when validating an offset-only body.
        cell_x: i64,
        /// Sample cell z, or zero when validating an offset-only body.
        cell_z: i64,
        /// Neighbor cell x or x offset.
        neighbor_cell_x: i64,
        /// Neighbor cell z or z offset.
        neighbor_cell_z: i64,
    },
    /// River/basin capacity was zero.
    #[error("inspect row `{id}` has zero capacity")]
    ZeroCapacity {
        /// Basin identity.
        id: StableId,
    },
    /// Vegetation exclusion radius was zero.
    #[error("vegetation inspect `{id}` exclusion radius must be positive")]
    ZeroExclusionRadius {
        /// Procedure identity.
        id: StableId,
    },
    /// Accepted placements exceeded evaluations.
    #[error("inspect row `{id}` accepts {accepts} of {samples} samples")]
    AcceptsExceedSamples {
        /// Field identity.
        id: StableId,
        /// Accepted count.
        accepts: u64,
        /// Evaluation count.
        samples: u64,
    },
    /// Record kind did not match the body tag.
    #[error("inspect kind `{kind:?}` does not match body `{body:?}`")]
    KindBodyMismatch {
        /// Declared kind.
        kind: WorldgenInspectKindV1,
        /// Body kind.
        body: WorldgenInspectKindV1,
    },
    /// Two samples shared one canonical `(kind, id)` key.
    #[error("worldgen inspect repeats `{kind:?}` `{id}`")]
    DuplicateRecord {
        /// Repeated kind.
        kind: WorldgenInspectKindV1,
        /// Repeated identity.
        id: StableId,
    },
    /// A sample was supplied for a kind the query did not request.
    #[error("worldgen inspect sample kind `{kind:?}` was not requested")]
    UnrequestedKind {
        /// Extra kind.
        kind: WorldgenInspectKindV1,
    },
    /// A sample was supplied for a kind with no accepted subscription.
    #[error("worldgen inspect collected `{kind:?}` without a subscription")]
    CollectionWithoutSubscription {
        /// Collected kind.
        kind: WorldgenInspectKindV1,
    },
    /// A sample cell lay outside the query window.
    #[error("inspect `{kind:?}` `{id}` cell ({cell_x}, {cell_z}) is outside the query window")]
    SampleOutsideQuery {
        /// Sample kind.
        kind: WorldgenInspectKindV1,
        /// Sample identity.
        id: StableId,
        /// Sample cell x.
        cell_x: i64,
        /// Sample cell z.
        cell_z: i64,
    },
    /// Per-kind sample arithmetic overflowed.
    #[error("worldgen inspect `{kind:?}` sample count overflowed")]
    SampleCountOverflow {
        /// Overflowing kind.
        kind: WorldgenInspectKindV1,
    },
    /// One kind exceeded the per-kind sample cap.
    #[error("worldgen inspect `{kind:?}` has {observed} samples; maximum is {maximum}")]
    SamplesPerKindExceeded {
        /// Kind at capacity.
        kind: WorldgenInspectKindV1,
        /// Observed count.
        observed: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// Even an empty report envelope exceeded the total byte cap.
    #[error("worldgen inspect envelope has {observed} bytes; maximum is {maximum}")]
    EnvelopeExceedsLimit {
        /// Observed encoded length.
        observed: usize,
        /// Configured maximum.
        maximum: usize,
    },
    /// Canonical JSON encoding failed.
    #[error(transparent)]
    Canonical(#[from] CanonicalJsonError),
    /// Intermediate JSON projection failed.
    #[error("failed to project worldgen inspect JSON: {source}")]
    Json {
        /// Serializer error.
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DiagnosticReportLimits, ReportConsentV1, SampleKey, SubscriptionConsumerId,
        SubscriptionOrigin, SubscriptionTarget, build_diagnostic_report,
    };
    use std::collections::HashMap;

    fn id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("stable ID `{value}`: {error}"))
    }

    fn hash(label: &str) -> CanonicalHash {
        CanonicalHash::digest(label.as_bytes())
    }

    fn bounds() -> WorldgenInspectBoundsV1 {
        WorldgenInspectBoundsV1::new(-4, -4, -1, -1)
            .unwrap_or_else(|error| panic!("bounds: {error}"))
    }

    fn query(requested: BTreeSet<WorldgenInspectKindV1>) -> WorldgenInspectQueryV1 {
        WorldgenInspectQueryV1 {
            engine_epoch: EngineEpoch::new(1),
            world_epoch: Some(WorldEpoch::new(2)),
            generation_input_hash: hash("input"),
            generation_provenance_hash: hash("provenance"),
            origin_cell_x: -3,
            origin_cell_z: -3,
            radius_cells: 2,
            requested,
        }
    }

    fn territory_record() -> WorldgenInspectRecordV1 {
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Territory,
            id("terrenia:territory-domain/temperate-woodland@1"),
            -3,
            -2,
            WorldgenInspectBodyV1::Territory(TerritoryInspectFactsV1 {
                winner: id("terrenia:biome/temperate-woodland"),
                runner_up: id("terrenia:biome/arid-badlands"),
                boundary_distance_voxels: 12,
                primary_owner: id("terrenia:worldgen/terrain-woodland@1"),
                secondary_owner: id("terrenia:worldgen/terrain-badlands@1"),
                transition_provider: id("terrenia:worldgen/transition-woodland-badlands@1"),
                transition_revision: 1,
                transition_width_voxels: 8,
                in_transition_band: true,
                adjacent: id("terrenia:biome/arid-badlands"),
                provenance: hash("territory-provenance"),
            }),
        )
        .unwrap_or_else(|error| panic!("territory record: {error}"))
    }

    fn river_record() -> WorldgenInspectRecordV1 {
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::River,
            id("latticeaxiom:hydrology-basin/surface-west@1"),
            -4,
            -3,
            WorldgenInspectBodyV1::River(RiverInspectFactsV1 {
                plan_id: id("latticeaxiom:hydrology-plan/overworld@1"),
                basin_id: id("latticeaxiom:hydrology-basin/surface-west@1"),
                connection_id: Some(id("latticeaxiom:hydrology-link/west-east@1")),
                bounds: bounds(),
                elevation_rank: 3,
                capacity_units: 64,
                dependency_receipt: hash("river-receipt"),
            }),
        )
        .unwrap_or_else(|error| panic!("river record: {error}"))
    }

    #[test]
    fn source_ids_are_stable_and_kind_ordered() {
        let ids: Vec<String> = WorldgenInspectKindV1::ALL
            .into_iter()
            .map(|kind| kind.source_id().map(|id| id.to_string()))
            .collect::<Result<_, _>>()
            .unwrap_or_else(|error| panic!("source IDs: {error}"));
        assert_eq!(
            ids,
            [
                WORLDGEN_TERRITORY_INSPECT_ID,
                WORLDGEN_BOUNDARY_INSPECT_ID,
                WORLDGEN_RIVER_INSPECT_ID,
                WORLDGEN_GEOLOGY_INSPECT_ID,
                WORLDGEN_RESOURCE_INSPECT_ID,
                WORLDGEN_VEGETATION_INSPECT_ID,
                WORLDGEN_SPAWN_INSPECT_ID,
                WORLDGEN_PORTAL_INSPECT_ID,
                WORLDGEN_ENTRANCE_INSPECT_ID,
                WORLDGEN_CAVE_OWNERSHIP_INSPECT_ID,
                WORLDGEN_CAVE_REJECTION_INSPECT_ID,
                WORLDGEN_CAVE_SDF_INSPECT_ID,
                WORLDGEN_CAVE_CONNECTIVITY_INSPECT_ID,
                WORLDGEN_FLUID_DECISION_INSPECT_ID,
                WORLDGEN_PLANNING_SEAM_INSPECT_ID,
            ]
        );
        assert_eq!(WorldgenInspectKindV1::NATURAL.len(), 7);
        assert_eq!(WorldgenInspectKindV1::CAVE_HYDROLOGY.len(), 8);
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
    }

    #[test]
    fn hashmap_insertion_order_cannot_change_report_bytes() {
        let mut map = HashMap::new();
        map.insert("river", river_record());
        map.insert("territory", territory_record());
        let first = WorldgenInspectSamplesV1::new(map.values().cloned())
            .unwrap_or_else(|error| panic!("samples: {error}"));
        let mut reversed = HashMap::new();
        reversed.insert("territory", territory_record());
        reversed.insert("river", river_record());
        let second = WorldgenInspectSamplesV1::new(reversed.values().cloned())
            .unwrap_or_else(|error| panic!("reversed samples: {error}"));
        let collection = WorldgenInspectCollectionV1::new(BTreeSet::from([
            WorldgenInspectKindV1::Territory,
            WorldgenInspectKindV1::River,
        ]));
        let requested = collection.subscribed().clone();
        let left = compile_worldgen_inspect_report(
            &query(requested.clone()),
            &first,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .unwrap_or_else(|error| panic!("left: {error}"));
        let right = compile_worldgen_inspect_report(
            &query(requested),
            &second,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .unwrap_or_else(|error| panic!("right: {error}"));
        assert_eq!(
            left.canonical_hash()
                .unwrap_or_else(|error| panic!("{error}")),
            right
                .canonical_hash()
                .unwrap_or_else(|error| panic!("{error}"))
        );
        assert_eq!(left.records[0].kind, WorldgenInspectKindV1::Territory);
        assert_eq!(left.records[1].kind, WorldgenInspectKindV1::River);
        assert!(left.query.origin_cell_x < 0);
        assert!(left.query.origin_cell_z < 0);
    }

    #[test]
    fn unsubscribed_kinds_are_not_collected() {
        let samples = WorldgenInspectSamplesV1::new([territory_record()])
            .unwrap_or_else(|error| panic!("samples: {error}"));
        let collection =
            WorldgenInspectCollectionV1::new(BTreeSet::from([WorldgenInspectKindV1::Territory]));
        let report = compile_worldgen_inspect_report(
            &query(BTreeSet::from([
                WorldgenInspectKindV1::Territory,
                WorldgenInspectKindV1::River,
                WorldgenInspectKindV1::Spawn,
            ])),
            &samples,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .unwrap_or_else(|error| panic!("report: {error}"));
        assert_eq!(
            report.skipped_kinds,
            BTreeSet::from([WorldgenInspectKindV1::River, WorldgenInspectKindV1::Spawn])
        );
        assert!(report.dedicated_work_enabled);
        assert_eq!(report.records.len(), 1);

        let leaked = WorldgenInspectSamplesV1::new([territory_record(), river_record()])
            .unwrap_or_else(|error| panic!("leaked samples: {error}"));
        let error = compile_worldgen_inspect_report(
            &query(BTreeSet::from([
                WorldgenInspectKindV1::Territory,
                WorldgenInspectKindV1::River,
            ])),
            &leaked,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .expect_err("unsubscribed river samples must fail closed");
        assert!(matches!(
            error,
            WorldgenInspectError::CollectionWithoutSubscription {
                kind: WorldgenInspectKindV1::River
            }
        ));
    }

    #[test]
    fn planner_subscription_enables_only_active_targets() {
        let mut planner = SubscriptionPlanner::default();
        let source = WorldgenInspectKindV1::Geology
            .source_id()
            .unwrap_or_else(|error| panic!("{error}"));
        planner
            .subscribe(
                SubscriptionTarget::new(source, SampleKey::new(0)),
                SubscriptionConsumerId::new(1),
                SubscriptionOrigin::ExplicitReport,
            )
            .unwrap_or_else(|error| panic!("subscribe: {error}"));
        let collection = WorldgenInspectCollectionV1::from_planner(&planner)
            .unwrap_or_else(|error| panic!("collection: {error}"));
        assert!(collection.allows(WorldgenInspectKindV1::Geology));
        assert!(!collection.allows(WorldgenInspectKindV1::Resource));
        assert_eq!(collection.subscribed().len(), 1);
    }

    #[test]
    fn radius_and_window_fail_closed() {
        let mut too_wide = query(BTreeSet::from([WorldgenInspectKindV1::Territory]));
        too_wide.radius_cells = MAX_WORLDGEN_INSPECT_RADIUS_CELLS + 1;
        assert!(matches!(
            too_wide.validate(),
            Err(WorldgenInspectError::RadiusOutOfRange { .. })
        ));

        let samples = WorldgenInspectSamplesV1::new([territory_record()])
            .unwrap_or_else(|error| panic!("samples: {error}"));
        let collection =
            WorldgenInspectCollectionV1::new(BTreeSet::from([WorldgenInspectKindV1::Territory]));
        let mut far = query(BTreeSet::from([WorldgenInspectKindV1::Territory]));
        far.origin_cell_x = 40;
        far.origin_cell_z = 40;
        let error = compile_worldgen_inspect_report(
            &far,
            &samples,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .expect_err("out-of-window samples must fail closed");
        assert!(matches!(
            error,
            WorldgenInspectError::SampleOutsideQuery { .. }
        ));
    }

    #[test]
    fn diagnostic_projection_redacts_coordinates_without_consent() {
        let samples = WorldgenInspectSamplesV1::new([territory_record()])
            .unwrap_or_else(|error| panic!("samples: {error}"));
        let collection =
            WorldgenInspectCollectionV1::new(BTreeSet::from([WorldgenInspectKindV1::Territory]));
        let report = compile_worldgen_inspect_report(
            &query(BTreeSet::from([WorldgenInspectKindV1::Territory])),
            &samples,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .unwrap_or_else(|error| panic!("report: {error}"));
        let inputs = report
            .diagnostic_inputs()
            .unwrap_or_else(|error| panic!("inputs: {error}"));
        let limits = DiagnosticReportLimits::new(32, 32, 8_192, 4_096)
            .unwrap_or_else(|error| panic!("limits: {error}"));
        let diagnostic = build_diagnostic_report(inputs, &ReportConsentV1::default(), limits)
            .unwrap_or_else(|error| panic!("diagnostic: {error}"));
        assert!(
            diagnostic
                .records
                .iter()
                .any(|row| matches!(row.value, crate::ReportValueV1::Redacted))
        );
        let consented = build_diagnostic_report(
            report
                .diagnostic_inputs()
                .unwrap_or_else(|error| panic!("inputs: {error}")),
            &ReportConsentV1::default().include(ReportSensitivityV1::Coordinate),
            limits,
        )
        .unwrap_or_else(|error| panic!("consented: {error}"));
        assert!(
            consented
                .records
                .iter()
                .any(|row| matches!(row.value, crate::ReportValueV1::Present(_)))
        );
    }

    #[test]
    fn record_cap_truncates_the_canonical_suffix() {
        let samples = WorldgenInspectSamplesV1::new([river_record(), territory_record()])
            .unwrap_or_else(|error| panic!("samples: {error}"));
        let collection = WorldgenInspectCollectionV1::new(BTreeSet::from([
            WorldgenInspectKindV1::Territory,
            WorldgenInspectKindV1::River,
        ]));
        let limits = WorldgenInspectLimits::new(1, DEFAULT_MAX_WORLDGEN_INSPECT_BYTES, 8)
            .unwrap_or_else(|error| panic!("limits: {error}"));
        let report = compile_worldgen_inspect_report(
            &query(collection.subscribed().clone()),
            &samples,
            &collection,
            limits,
        )
        .unwrap_or_else(|error| panic!("report: {error}"));
        assert_eq!(report.records.len(), 1);
        assert_eq!(report.records[0].kind, WorldgenInspectKindV1::Territory);
        assert_eq!(report.truncated_records, 1);
    }

    fn portal_record() -> WorldgenInspectRecordV1 {
        WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::Portal,
            id("latticeaxiom:cave-portal/limestone-crystal@1"),
            -3,
            -3,
            WorldgenInspectBodyV1::Portal(PortalInspectFactsV1 {
                portal_hash: hash("portal"),
                first_domain: id("terrenia:cave-topology-domain/crystal"),
                second_domain: id("terrenia:cave-topology-domain/limestone"),
                neighbor_cell_x: 1,
                neighbor_cell_z: 0,
                position_millimeters: [-1_000, 2_000, -3_000],
                tangent_axis: CaveInspectAxisV1::Y,
                clearance_width_millimeters: 1_000,
                clearance_height_millimeters: 2_000,
                fluid: PortalInspectFluidV1::Dry,
                dependency_receipt: hash("portal-receipt"),
            }),
        )
        .unwrap_or_else(|error| panic!("portal record: {error}"))
    }

    #[test]
    fn cave_hydrology_kinds_compile_without_natural_kinds() {
        let samples = WorldgenInspectSamplesV1::new([portal_record()])
            .unwrap_or_else(|error| panic!("samples: {error}"));
        let collection =
            WorldgenInspectCollectionV1::new(BTreeSet::from([WorldgenInspectKindV1::Portal]));
        let report = compile_worldgen_inspect_report(
            &query(BTreeSet::from([WorldgenInspectKindV1::Portal])),
            &samples,
            &collection,
            WorldgenInspectLimits::default(),
        )
        .unwrap_or_else(|error| panic!("report: {error}"));
        assert_eq!(report.records.len(), 1);
        assert_eq!(report.records[0].kind, WorldgenInspectKindV1::Portal);
        assert!(report.query.origin_cell_x < 0);
    }

    #[test]
    fn non_cardinal_seam_fails_closed() {
        let error = WorldgenInspectRecordV1::new(
            WorldgenInspectKindV1::PlanningSeam,
            id("latticeaxiom:planning-seam/invalid@1"),
            -3,
            -3,
            WorldgenInspectBodyV1::PlanningSeam(PlanningSeamInspectFactsV1 {
                neighbor_cell_x: 1,
                neighbor_cell_z: 1,
                shared_face_match: true,
                required_cave_portals: Vec::new(),
                fluid_discontinuity: false,
                seam_signature: hash("seam"),
            }),
        )
        .expect_err("diagonal seam must fail closed");
        assert!(matches!(
            error,
            WorldgenInspectError::NonCardinalSeam { .. }
        ));
    }

    #[test]
    fn report_dtos_deny_unknown_fields() {
        let json = r#"{
            "schema":"latticeaxiom:schema/worldgen-inspect-report@1",
            "schema_version":1,
            "query":{
                "engine_epoch":1,
                "world_epoch":2,
                "generation_input_hash":"00",
                "generation_provenance_hash":"00",
                "origin_cell_x":0,
                "origin_cell_z":0,
                "radius_cells":1,
                "requested":["territory"]
            },
            "records":[],
            "skipped_kinds":[],
            "truncated_records":0,
            "limits":{"max_records":1,"max_bytes":8,"max_samples_per_kind":1},
            "dedicated_work_enabled":false,
            "surprise":true
        }"#;
        assert!(serde_json::from_str::<WorldgenInspectReportV1>(json).is_err());
    }
}
