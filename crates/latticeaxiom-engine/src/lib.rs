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
//! [`latticeaxiom_storage::MemoryTransactionKernel`], streams a bounded chunk
//! working set around the local player, and presents chunk meshes rather than
//! one entity per block. The default client binary boots that host from a
//! reopened lock. The `playable` module remains a non-production fixture
//! reachable only through the `latticeaxiom-playable-fixture` extra binary.

#[cfg(feature = "client")]
mod client;
mod host;
mod instance;
#[cfg(feature = "client")]
mod playable;
mod prepared;
#[cfg(feature = "client")]
mod presentation_fixture;

#[cfg(feature = "client")]
pub use client::{
    ProductionClientError, load_lock_verified_images, run_client_host_from_lock,
    run_client_host_from_workspace,
};
pub use host::{
    CaveOccupancyArbitrationV1, CellOccupancyV1, ChunkFaceV1, ChunkLifecycle, ChunkMeshCursor,
    ChunkPresentation, HOTBAR_SLOTS, INVENTORY_SLOTS, ProductionHostError, ProductionHostPlugin,
    ProductionInspectSurface, ProductionInventoryView, ProductionMemoryStart,
    ProductionMemoryStartError, ProductionPlayerPose, ProductionSessionPause, ProductionSpine,
    ProductionWorldList, ProductionWorldStorage, RequiredCaveEntranceV1, SealedWorldWriterHost,
    SealedWriterHostError, WorkingSetDiagnosticsV1, authored_content_catalog,
    authored_gameplay_catalog, empty_gameplay_catalog, sealed_activation_binding,
};
pub use instance::{
    EngineInstance, EngineInstanceError, EngineProfile, FixedTickCount, MAX_TICKS_PER_ADVANCE,
    VerifiedProductLockHash,
};
pub use latticeaxiom_content::{FluidFlowV1, FluidLevelV1, FluidStateV1};
pub use latticeaxiom_gameplay::{
    CommandOutcomeV1, ContainerId, DropEntityId, GameplayCatalog, GameplayReject, ItemId,
    ItemStackV1, ItemStateV1, RecipeId, SlotIndex, WorkstationId,
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
#[cfg(feature = "client")]
pub use playable::{PlayableClientError, run_playable_client};
pub use prepared::{
    CallbackContext, CatalogKind, LockVerifiedComposeImages, PreparationError,
    StructurallyValidatedComposeImages,
};
#[cfg(feature = "client")]
pub use presentation_fixture::run_temporary_client_presentation_fixture;
