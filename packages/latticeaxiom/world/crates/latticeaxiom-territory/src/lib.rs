//! Deterministic, bounded territory and cave-planning contracts for D7.
//!
//! This crate extends the D4 query boundary from `latticeaxiom-worldgen`
//! without owning execution, persistence, rendering, fluids, or Bevy runtime
//! services. It compiles pure data into an immutable territory plan. Bevy task
//! pools remain responsible for scheduling independent queries.

mod atlas;
mod bounds;
mod cave;
mod diagnostics;
mod epoch;
mod error;
mod hashes;
mod hydrology;
mod provider;
mod query;
mod receipt;

pub use atlas::{
    AtlasConfigV1, AtlasScaleV1, AtlasStatisticsV1, SurfaceTerritoryCandidateV1,
    TerritoryAreaStatisticsV1, TerritoryPlanInputV1, TerritoryPlanV1, TerritoryQueryLevelV1,
    TerritoryQueryV1,
};
pub use bounds::{PlanningCellBoundsV1, VerticalRangeV1};
pub use cave::{
    AxisV1, CaveAdjacencyV1, CaveDomainAlgorithmV1, CaveMustConnectDestinationV1,
    CavePassabilityReceiptV1, CavePortalV1, CaveSurfaceEntranceV1, CaveTopologyDomainIdV1,
    CaveTopologyEdgeV1, CaveTopologyNodeKindV1, CaveTopologyNodeV1, CaveTopologyParentV1,
    CaveTopologyPlanV1, PortalAssertionV1, PortalHydrologyContractV1, UndergroundTerritoryV1,
};
pub use diagnostics::TerritoryConflictDiagnosticV1;
pub use epoch::{
    CellEpochAssignmentV1, CellEpochFreezeReceiptV1, PlanningCellEpochLedgerV1,
    PlanningCellTransitionAdapterV1, PlanningCellTransitionReceiptV1,
};
pub use error::{TerritoryError, TerritoryResult};
pub use hashes::{
    AtlasPlanHashV1, CaveEntranceIdV1, CavePassabilityReceiptHashV1, CavePortalIdV1,
    CaveTopologyNodeIdV1, CaveTopologyPlanHashV1, HydrologyPlanHashV1,
    TerritoryConflictDiagnosticHashV1, TerritoryDomainIdV1, TerritoryPlanReceiptHashV1,
    TransitionReceiptHashV1,
};
pub use hydrology::{
    CardinalDirectionV1, HydrologyBasinV1, HydrologyConnectionV1, HydrologyPlanV1,
};
pub use provider::{
    ContributionBudgetV1, ContributionChannelV1, ContributionCompositorV1, ContributionTargetV1,
    CoordinatorOfferV1, PrimaryChannelV1, PrimaryOwnershipDomainV1, PrimaryProviderOfferV1,
    ResolvedPrimaryOwnerV1, SpatialContributionV1, TerritoryLimitsV1,
};
pub use query::{
    OrderedOwnershipCandidateV1, SurfaceTerritoryQueryV1, TerritoryQueryCoverageV1,
    UndergroundTerritoryQueryV1,
};
pub use receipt::TerritoryPlanReceiptV1;

pub use latticeaxiom_worldgen::{
    CaveTopologyAlgorithmV1, CaveTopologyLayerInputV1, CaveVoxelPassabilityReceiptV1,
    ChunkCoordinate, DimensionId, GenerationEpochIdV1, LockedClosureFingerprintV1,
    PlanningCellCoordinateV1, ProviderGenerationIdentityV1, WorldSeedV1, WorldgenConfigHashV1,
    WorldgenConfigV1,
};
