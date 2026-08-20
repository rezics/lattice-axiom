//! Bounded runtime adapter over storage-committed voxel projections.
//!
//! The crate projects storage receipts or snapshots into a discardable resident
//! cache, captures immutable chunk-plus-halo derived inputs, enforces queue and
//! retained-memory backpressure, rejects stale completions, exposes conservative
//! collider fallback state, and performs Y-up committed-state DDA selection.
//! It is not a persistence authority, renderer facade, task runtime, or physics
//! backend and cannot author voxel edits or advance authoritative revisions.

mod dda;
mod diagnostics;
mod error;
mod model;
mod queue;
mod working_set;

pub use dda::{CellSelection, DdaCell, DdaOrigin, DdaOutcome, DdaQuery, DdaUnavailable};
pub use diagnostics::{QueueDiagnostics, RuntimeDiagnostics};
pub use error::{RuntimeError, RuntimeResult};
pub use model::{
    ApplyByteDeclaration, BackpressureReason, CancellationAckOutcome, CancellationReleaseReceipt,
    CancellationRequest, CancellationToken, ColliderFailure, ColliderSafetyState,
    ColliderSemanticFingerprint, CollisionSemantics, CommittedChunkProjection, CompletionOutcome,
    DerivedApplyReceipt, DerivedEnqueueReceipt, DerivedInput, DerivedJobId, DerivedJobKey,
    DerivedKind, DerivedMemoryBudget, DerivedOwner, DerivedPriority, DerivedQueueLimits,
    DerivedRequest, DerivedRequestSet, DerivedSemanticFingerprint, DerivedSourceFingerprint,
    DerivedTicket, DispatchOutcome, EnqueueDecision, EvictionLeaseGeneration, EvictionPermit,
    EvictionReceipt, FixedTick, InterestWindow, LocalVoxelCoordinate,
    MAX_AUTHORITATIVE_REACH_METERS, MemoryStage, MeshSemanticFingerprint, NeighborRevision,
    NeighborRevisions, ProjectionDecision, ProjectionEvidence, ProjectionReceipt, RetainedBytes,
    RuntimeGeneration, RuntimeLimits, StaleReason, VoxelCoordinate, WorkingSetScope, WorldEpoch,
};
pub use working_set::VoxelRuntime;

#[cfg(test)]
mod tests;
