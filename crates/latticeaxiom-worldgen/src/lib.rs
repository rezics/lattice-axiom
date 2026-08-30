//! Deterministic, bounded world-generation contracts.
//!
//! The crate is an engine-independent algorithm scaffold. It resolves one
//! coordinator/provider identities and package-owned surface-biome terrain
//! programs into an immutable [`GenerationPlanV1`], then produces provisional snapshot
//! candidates without opening a writer. Bevy owns task execution, while the
//! storage layer owns atomic durable publication. Package/RegistrationImage
//! graph compilation and a verified cross-epoch adapter remain external gates.
//!
//! Terrain uses Bevy-native right-handed Y-up coordinates. Chunk coordinates
//! are the canonical [`latticeaxiom_storage::ChunkCoordinate`] type and voxel
//! coordinates are ordered `(x, y, z)`. Generation never reads a materialized
//! neighbor chunk, so chunk and task completion order cannot affect output.
//!
//! Surface ecology is selected before terrain dispatch. Each enabled ecology
//! must supply exactly one [`SurfaceBiomeTerrainProgramV1`]; the platform does
//! not contain product-specific terrain provider slots. The coarse selector
//! implements the D7 [`TerritoryQueryV1`] result shape, not a full Territory Atlas.
//!
//! The optional V5 natural layer adds a third surface style, queryable strata,
//! stable resource fields, exclusion-radius vegetation, and surface river/basin
//! planning on the same coordinator. The optional V6 cave-topology layer adds
//! domain-owned corridors, a bounded branch contributor, and passability
//! receipts without replacing D4 occupancy. The optional V6 hydrology occupancy
//! layer adds sea-level surface water, underground drainage, aquifer tables,
//! and initial water/lava occupancy candidates without changing snapshot
//! schema.

mod cave;
mod cave_topology;
mod config;
mod epoch;
mod error;
mod fixed_field;
mod generation;
mod hashes;
mod hydrologic_domain;
mod hydrologic_topology;
mod hydrology;
mod landscape_evolution;
mod natural;
mod provider;
mod region;
mod roles;
mod seed;
mod semantic_terrain;
mod spawn;
mod terrain_config;
mod terrain_field;
mod terrain_program;
mod territory;

pub use cave::{
    CaveFaceFieldRequestV1, CaveFaceOccupancyValidationV1, CaveFieldPortalAssertionV1,
    CaveFieldPortalPlanV1, CaveOccupancyArbitrationV1, ChunkFaceV1, SharedFaceKeyV1,
};
pub use cave_topology::{
    CaveBranchContributorV1, CaveLayerCorridorV1, CaveLayerEntranceV1, CaveLayerPortalV1,
    CaveOwnedDomainV1, CaveTopologyAlgorithmV1, CaveTopologyLayerInputV1,
    CaveVoxelPassabilityReceiptV1, cell_center_voxels, cell_center_voxels_at_edge,
    millimeters_to_voxels,
};
pub use config::{WorldgenConfigV1, WorldgenLimitsV1};
pub use epoch::{
    AdjacentCellEpochStateV1, AdjacentCellEpochV1, AdjacentEpochSnapshotV1,
    BoundaryAdapterDeclarationV1, BoundaryReceiptV1, CellEpochStateV1, ExistingSnapshotEvidenceV1,
    PlanningCellCoordinateV1,
};
pub use error::{WorldgenError, WorldgenResult};
pub use fixed_field::{
    AuthoritativeFieldAlgorithmV1, FIXED_FIELD_FRACTION_BITS_V1, FIXED_FIELD_OUTPUT_BITS_V1,
    FixedCoordinateV1, FixedFieldSampleV1, MAX_FIXED_FIELD_INTEGER_COORDINATE_V1,
    open_simplex_2f_3d_v1, open_simplex_2s_2d_v1,
};
pub use generation::{
    ChunkDraftV1, ChunkGenerationOutcomeV1, ChunkGenerationRequestV1, D4SnapshotCandidateV1,
    GenerationDiagnosticsV1, GenerationPlanInputV1, GenerationPlanV1, GenerationReceiptV1,
    PlacementPredicateKindV1, PlacementPredicateReceiptV1, RoleBindingReceiptV1,
};
pub use hashes::{
    AquiferBasinIdV1, BoundaryIdV1, CaveTopologyLayerHashV1, DrainageLinkIdV1, GenerationEpochIdV1,
    GenerationInputHashV1, GenerationProvenanceHashV1, GeneratorFingerprintV1, HydrologicBasinIdV1,
    HydrologicBoundarySignatureV1, HydrologicDomainConfigHashV1, HydrologicDomainIdV1,
    HydrologicDomainInputHashV1, HydrologicDomainPlanHashV1, HydrologicOutletIdV1,
    HydrologicPortIdV1, HydrologicTopologyHashV1, HydrologyOccupancyHashV1,
    LandscapeEvolutionConfigHashV1, LandscapeEvolutionPlanHashV1, LockedClosureFingerprintV1,
    NaturalLayerHashV1, PlanActivationIdV1, PlanningCellIdV1, RiverBasinIdV1, RiverSegmentIdV1,
    SemanticTerrainPlanHashV1, SemanticTerrainPolicyHashV1, SharedFaceHashV1, SnapshotChecksumV1,
    StaticReservoirHashV1, TerrainBoundaryAdapterHashV1, TerrainConfigHashV2, WaterBodyIdV1,
    WorldgenConfigHashV1,
};
pub use hydrologic_domain::{
    DepressionClassV1, DepressionHierarchyV1, DepressionRecordV1, DevelopmentRoutingComparisonV1,
    HYDROLOGIC_WEIGHT_SCALE_V1, HydrologicBoundaryEdgeV1, HydrologicBoundaryPortV1,
    HydrologicDomainAccountingV1, HydrologicDomainAlgorithmV1, HydrologicDomainCacheKeyV1,
    HydrologicDomainCacheV1, HydrologicDomainConfigV1, HydrologicDomainGridV1,
    HydrologicDomainInputV1, HydrologicDomainPlanV1, HydrologicGridCoordinateV1,
    HydrologicPortKindV1, MfdReceiverV1, MfdRoutingCellV1, d_infinity_development_comparison_v1,
    plan_hydrologic_domain_v1, plan_hydrologic_domain_with_cancellation_v1,
    plan_hydrologic_domains_parallel_v1,
};
pub use hydrologic_topology::{
    BasinRecordV1, HydrologicTopologyAccountingV1, HydrologicTopologyConfigV1,
    HydrologicTopologyPlanV1, OutletKindV1, OutletRecordV1, QuantizedRiverPointV1, RiverEndpointV1,
    RiverSdfSampleV1, RiverSegmentV1, StaticReservoirChunkV1, StaticReservoirFlowV1,
    StaticReservoirSamplerV1, StaticReservoirVoxelV1, StaticWaterColumnV1, WaterBodyKindV1,
    WaterBodyV1, WaterSurfaceModelV1, build_hydrologic_topology_v1,
    materialize_static_reservoir_chunk_v1,
};
pub use hydrology::{
    AquiferSampleV1, DrainageSampleV1, HydrologyAccountingV1, HydrologyFaceContinuityV1,
    HydrologyFlowV1, HydrologyFluidBindingsV1, HydrologyOccupancyCandidateV1,
    HydrologyOccupancyCellV1, HydrologyOccupancyConfigV1, HydrologyOccupancyInputV1,
    HydrologyOccupancyKindV1, HydrologyOccupancySampleV1,
};
pub use landscape_evolution::{
    LandscapeEvolutionAccountingV1, LandscapeEvolutionConfigV1, LandscapeEvolutionEvidenceV1,
    LandscapeEvolutionInputV1, LandscapeEvolutionPlanV1, evolve_hydrologic_landscape_v1,
    evolve_hydrologic_landscape_with_cancellation_v1, evolve_hydrologic_landscapes_parallel_v1,
};
pub(crate) use natural::NaturalSamplerV1;
pub use natural::{
    GeologicSampleV1, NaturalLayerConfigV1, NaturalLayerInputV1, ResourceFieldSampleV1,
    RiverSampleV1,
};
pub use provider::{ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1};
pub use region::{
    BoundedGeneratedRegionV1, MAX_BOUNDED_REGION_CHUNKS, ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1,
};
pub use roles::{
    D4_MAX_CATALOG_BLOCK_COUNT, D4_MAX_ROLE_BINDING_COUNT, D4_MINIMUM_BLOCK_COUNT,
    D4BlockCatalogClosureV1, D4MaterialRoleV1, D4RoleVocabularyV1, D7_NATURAL_BLOCK_COUNT,
    FrozenRoleBindingsV1, NaturalRoleVocabularyV1,
};
pub use seed::{WorldSeedV1, WorldgenSeedRootV2};
pub use semantic_terrain::{
    ClosedSplinePointV1, ClosedSplineV1, HydrologyConstrainedTerrainSamplerV1,
    SemanticDensitySampleV1, SemanticFieldSampleV1, SemanticFieldSpecV1,
    SemanticHydrologicTerrainInputV1, SemanticHydrologicTerrainPlanV1, SemanticTerrainFieldV1,
    SemanticTerrainPolicyV1, TerrainBoundaryAdapterEvidenceV1,
    build_semantic_hydrologic_terrain_plan_v1,
};
pub use spawn::{
    AuthoredWorldgenBindingsV1, SpawnCellInspectionV1, SpawnCellOverrideV1, SpawnLocationV1,
    SpawnOccupancyViewV1, SpawnRejectV1, SpawnSearchBoundsV1, evaluate_spawn_column,
    inspect_spawn_cell, required_spawn_chunks, select_safe_spawn, select_safe_spawn_prefer_style,
};
pub use terrain_config::{
    ClimateConfigV2, LandmassConfigV2, ReliefConfigV2, SurfaceWaterConfigV2, TerrainConfigV2,
    UndergroundConfigV2, WorldBoundsV2,
};
pub use terrain_field::{TerrainColumnSampleV2, TerrainFamilyV2};
pub use terrain_program::{
    BiomeSelectionRuleV1, SurfaceBiomeIdV1, SurfaceBiomeTerrainProgramV1, SurfaceTerrainDomainV1,
    TerrainBaseAlgorithmV1,
};
pub use territory::{TerrainStyleV1, TerritoryQueryV1, TransitionMetadataV1};

pub use latticeaxiom_storage::{
    ChunkCoordinate, ChunkRevision, ChunkRevisionExpectation, DimensionId,
};
