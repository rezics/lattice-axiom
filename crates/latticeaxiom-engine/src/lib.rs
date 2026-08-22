//! Bevy host scaffolding for receipt-verified Lattice Axiom images.
//!
//! Ordinary launch reopens and fully verifies `latticeaxiom.lock` before this
//! crate constructs [`latticeaxiom_compose::RuntimeImage`] or a Bevy
//! [`bevy::app::App`]. Client [`bevy::prelude::DefaultPlugins`] and GPU-free
//! headless hosts share that reopened lock and must not re-resolve. Compiled
//! registration evidence is transported alongside the lock and rebound to it.
//! Native modules are never mapped on this path. `RuntimeImage` still carries
//! callback keys rather than signature or loaded-code attestations, so actual
//! adapter installation remains a later fail-closed boundary. Each instance
//! owns one Bevy [`bevy::app::App`].
//!
//! The production playable spine lives in [`host`]. It starts from
//! [`LockVerifiedComposeImages`], stores voxels through
//! [`latticeaxiom_storage::MemoryTransactionKernel`] as the session cache,
//! streams a bounded chunk working set around the local player, and presents
//! chunk meshes rather than one entity per block. Durable Save & Quit is
//! sealed-receipt gated. The default client binary boots that host from a
//! reopened lock. The `playable` module remains a non-production fixture
//! reachable only through the `latticeaxiom-playable-fixture` extra binary.

#[cfg(feature = "client")]
mod client;
mod host;
mod input;
mod instance;
#[cfg(feature = "client")]
mod playable;
mod prepared;
#[cfg(feature = "client")]
mod presentation_fixture;
mod settings;
#[cfg(feature = "client")]
mod supervisor;

#[cfg(feature = "client")]
pub use client::{
    ProductionClientError, load_lock_verified_images, load_lock_verified_images_from,
    run_client_host_from_lock, run_client_host_from_workspace,
};
pub use host::{
    ADR_0026_ACTIVE_COVERAGE_M, ADR_0026_CHUNK_EDGE_VOXELS, ADR_0026_RESIDENT_COVERAGE_M,
    CaveOccupancyArbitrationV1, CellOccupancyV1, ChildResultV1, ChunkFaceV1, ChunkLifecycle,
    ChunkMeshCursor, ChunkPresentation, ContentDisplayCatalogV1, ContentDisplayLabelV1,
    DerivedQueueSnapshotV1, HOTBAR_SLOTS, HostFluidTickV1, INVENTORY_SLOTS, ProductionHostError,
    ProductionHostPlugin, ProductionInspectSurface, ProductionInventoryView, ProductionMemoryStart,
    ProductionMemoryStartError, ProductionPlayerPose, ProductionSessionPause, ProductionSpine,
    ProductionSurfaceRouter, ProductionWorldList, ProductionWorldStorage, RenderDeviceLost,
    RequiredCaveEntranceV1, STREAMING_PROFILE_EVIDENCE_SCHEMA_V1, SealedWorldWriterHost,
    SealedWriterHostError, StreamingProfileCountsV1, StreamingProfileEvidenceV1,
    WorkingSetDiagnosticsV1, WorldgenInspectReportV1, authored_content_catalog,
    authored_content_display_catalog, authored_gameplay_catalog, coverage_m,
    empty_gameplay_catalog, lock_selected_gameplay_catalog, radius_for_coverage,
    sealed_activation_binding,
};
pub use input::{HostInputError, compile_lock_selected_input, graph_selects_input_actions};
pub use instance::{
    EngineInstance, EngineInstanceError, EngineProfile, FixedTickCount, MAX_TICKS_PER_ADVANCE,
    VerifiedProductLockHash,
};
pub use latticeaxiom_content::{FluidFlowV1, FluidLevelV1, FluidStateV1};
pub use latticeaxiom_gameplay::{
    AuthorityTick, CommandOutcomeV1, ContainerId, ContainerStateV1, DropEntityId, GameplayCatalog,
    GameplayReject, InventoryInspectV1, ItemId, ItemStackV1, ItemStateV1, MiningInspectV1,
    PlayerId, ProcessId, RecipeId, RecipeInspectV1, SlotIndex, TransferCommandV1,
    TransferDirectionV1, WorkstationId,
};
pub use latticeaxiom_player::{
    ActionAxis2V1, ActionFrameInbox, ActionFrameInboxError, BlockEditReceiptV1, BlockEditSuccessV1,
    HeadlessTargetInspectV1, PlayerActionButtonsV1, PlayerActionFrameV1, PlayerActionV1,
    TargetInspectReceiptV1, TargetInspectRejectV1,
};
pub use latticeaxiom_storage::{
    AuthoritativeTransactionKernel, ChunkCoordinate, ChunkRevision, MemoryTransactionKernel,
};
pub use latticeaxiom_voxel_mesh::{Face, MeshReceipt};
pub use latticeaxiom_voxel_runtime::{
    FluidRevisionStamp, FluidRuntimeError, admit_fluid_completion,
};
#[cfg(feature = "client")]
pub use playable::{PlayableClientError, run_playable_client};
pub use prepared::{
    CallbackContext, CatalogKind, LockVerifiedComposeImages, PreparationError,
    StructurallyValidatedComposeImages,
};
#[cfg(feature = "client")]
pub use presentation_fixture::run_temporary_client_presentation_fixture;
pub use settings::{HostSettingsError, HostUserSettings};
#[cfg(feature = "client")]
pub use supervisor::{
    ProductSupervisorError, publish_child_exit, run_product_supervisor_from_workspace,
};
