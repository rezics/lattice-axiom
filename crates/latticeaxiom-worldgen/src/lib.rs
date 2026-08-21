//! Deterministic, bounded world-generation contracts for the D4 vertical slice.
//!
//! The crate is an engine-independent algorithm scaffold. It resolves one
//! coordinator/provider identity for every fixed D4 reference slot into an
//! immutable [`GenerationPlanV1`], then produces provisional snapshot
//! candidates without opening a writer. Bevy owns task execution, while the
//! storage layer owns atomic durable publication. Package/RegistrationImage
//! graph compilation and a verified cross-epoch adapter remain external gates.
//!
//! Terrain uses Bevy-native right-handed Y-up coordinates. Chunk coordinates
//! are the canonical [`latticeaxiom_storage::ChunkCoordinate`] type and voxel
//! coordinates are ordered `(x, y, z)`. Generation never reads a materialized
//! neighbor chunk, so chunk and task completion order cannot affect output.
//!
//! The two D4 styles are deterministic fixture algorithms (temperate woodland
//! and arid badlands); package-owned style identities and Predicate-receipt
//! registration schemas are not frozen here. The coarse selector implements
//! only the D7 [`TerritoryQueryV1`] result shape, not the full Territory Atlas.

mod cave;
mod config;
mod epoch;
mod error;
mod generation;
mod hashes;
mod provider;
mod region;
mod roles;
mod seed;
mod spawn;
mod territory;

pub use cave::{
    CaveFaceFieldRequestV1, CaveFaceOccupancyValidationV1, CaveFieldPortalAssertionV1,
    CaveFieldPortalPlanV1, CaveOccupancyArbitrationV1, ChunkFaceV1, SharedFaceKeyV1,
};
pub use config::{WorldgenConfigV1, WorldgenLimitsV1};
pub use epoch::{
    AdjacentCellEpochStateV1, AdjacentCellEpochV1, AdjacentEpochSnapshotV1,
    BoundaryAdapterDeclarationV1, CellEpochStateV1, ExistingSnapshotEvidenceV1,
    PlanningCellCoordinateV1,
};
pub use error::{WorldgenError, WorldgenResult};
pub use generation::{
    ChunkDraftV1, ChunkGenerationOutcomeV1, ChunkGenerationRequestV1, D4SnapshotCandidateV1,
    GenerationDiagnosticsV1, GenerationPlanInputV1, GenerationPlanV1, GenerationReceiptV1,
    PlacementPredicateKindV1, PlacementPredicateReceiptV1, RoleBindingReceiptV1,
};
pub use hashes::{
    BoundaryIdV1, GenerationEpochIdV1, GenerationInputHashV1, GenerationProvenanceHashV1,
    GeneratorFingerprintV1, LockedClosureFingerprintV1, PlanActivationIdV1, PlanningCellIdV1,
    SharedFaceHashV1, SnapshotChecksumV1, WorldgenConfigHashV1,
};
pub use provider::{ProviderGenerationIdentityV1, ProviderOfferV1, ProviderSlotV1};
pub use region::{
    BoundedGeneratedRegionV1, MAX_BOUNDED_REGION_CHUNKS, ORIGIN_NEIGHBORHOOD_CHUNK_COORDINATES_V1,
};
pub use roles::{
    D4_MAX_CATALOG_BLOCK_COUNT, D4_MAX_ROLE_BINDING_COUNT, D4BlockCatalogClosureV1,
    D4MaterialRoleV1, D4RoleVocabularyV1, FrozenRoleBindingsV1,
};
pub use seed::WorldSeedV1;
pub use spawn::{
    AuthoredWorldgenBindingsV1, SpawnCellInspectionV1, SpawnCellOverrideV1, SpawnLocationV1,
    SpawnOccupancyViewV1, SpawnRejectV1, SpawnSearchBoundsV1, evaluate_spawn_column,
    inspect_spawn_cell, required_spawn_chunks, select_safe_spawn,
};
pub use territory::{TerrainStyleV1, TerritoryQueryV1, TransitionMetadataV1};

pub use latticeaxiom_storage::{
    ChunkCoordinate, ChunkRevision, ChunkRevisionExpectation, DimensionId,
};
