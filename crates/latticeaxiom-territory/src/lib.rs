//! Deterministic, bounded territory and cave-planning contracts for D7.
//!
//! This crate extends the D4 query boundary from `latticeaxiom-worldgen`
//! without owning execution, persistence, rendering, fluids, or Bevy runtime
//! services. It compiles pure data into an immutable territory plan. Bevy task
//! pools remain responsible for scheduling independent queries.

mod atlas;
mod bounds;
mod cave;
mod epoch;
mod error;
mod hashes;
mod hydrology;
mod provider;

pub use atlas::{
    AtlasConfigV1, AtlasScaleV1, AtlasStatisticsV1, SurfaceTerritoryCandidateV1,
    TerritoryAreaStatisticsV1, TerritoryPlanInputV1, TerritoryPlanV1, TerritoryQueryLevelV1,
    TerritoryQueryV1,
};
pub use bounds::{PlanningCellBoundsV1, VerticalRangeV1};
pub use cave::{
    AxisV1, CaveAdjacencyV1, CavePortalV1, CaveTopologyDomainIdV1, CaveTopologyParentV1,
    PortalHydrologyContractV1, UndergroundTerritoryV1,
};
pub use epoch::{
    CellEpochAssignmentV1, CellEpochFreezeReceiptV1, PlanningCellEpochLedgerV1,
    PlanningCellTransitionAdapterV1, PlanningCellTransitionReceiptV1,
};
pub use error::{TerritoryError, TerritoryResult};
pub use hashes::{
    AtlasPlanHashV1, CavePortalIdV1, HydrologyPlanHashV1, TerritoryDomainIdV1,
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

pub use latticeaxiom_worldgen::{
    DimensionId, GenerationEpochIdV1, PlanningCellCoordinateV1, ProviderGenerationIdentityV1,
    WorldSeedV1,
};
