//! Product storage boundary independent from a physical database API.

use latticeaxiom_core::WorldId;
use latticeaxiom_storage::ChunkKey;

use crate::{
    CheckpointId, CheckpointOutcomeV1, CheckpointReceiptV1, CheckpointRequestV1,
    HeaderRepairPermitV1, PersistedChunkV1, StorageDurabilityCapabilityV1, WorldCommitOutcomeV1,
    WorldCommitRequestV1, WorldCreateOutcomeV1, WorldCreateRequestV1, WorldDbResult,
    WorldFrontierV1, WorldStorageLimitsV1, WorldStoragePreflightV1, WriterActivationV1,
};

/// Product boundary for authoritative materialized-world persistence.
///
/// Read-only preflight does not activate a writer. A caller may obtain a
/// [`WorldWriter`] only by presenting the immutable activation permit returned
/// by a successful preflight after all external module and decoder validation
/// has completed.
pub trait WorldStorage: Send + Sync {
    /// Returns the strongest physical persistence boundary this backend can prove.
    #[must_use]
    fn durability_capability(&self) -> StorageDurabilityCapabilityV1;

    /// Returns hard ceilings enforced before allocation or publication.
    #[must_use]
    fn limits(&self) -> WorldStorageLimitsV1;

    /// Provisions revision-zero authoritative metadata before a writer exists.
    ///
    /// The database publication is authoritative. A sidecar failure is
    /// represented in the returned outcome and does not pretend to roll back
    /// the database.
    ///
    /// # Errors
    ///
    /// Returns a typed error for an existing world or store identity, invalid
    /// metadata, exceeded limits, or a pre-publication database fault.
    fn provision_world(&self, request: WorldCreateRequestV1)
    -> WorldDbResult<WorldCreateOutcomeV1>;

    /// Performs bounded, read-only storage preflight.
    ///
    /// # Errors
    ///
    /// Returns a typed error when authoritative database metadata cannot be
    /// read or validated. A missing, stale, or corrupt sidecar is represented
    /// in the returned status and never overwrites database metadata.
    fn preflight(&self, world: WorldId) -> WorldDbResult<WorldStoragePreflightV1>;

    /// Captures a bounded, immutable read snapshot for one world.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the world does not exist or its metadata
    /// cannot be decoded and validated. Subsequent reads observe one coherent
    /// captured revision even while the authoritative writer advances.
    fn begin_read(&self, world: WorldId) -> WorldDbResult<Box<dyn WorldReadView>>;

    /// Repairs a missing, stale, or corrupt sidecar before writer activation.
    ///
    /// The permit is issued only by read-only preflight and is revalidated
    /// against authoritative DB metadata before publication.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the DB evidence changed or cannot be read.
    /// Publisher failure remains a recoverable status in the returned outcome.
    fn repair_header(&self, permit: HeaderRepairPermitV1) -> WorldDbResult<WorldCommitOutcomeV1>;

    /// Opens the world's single writer after revalidating an activation permit.
    ///
    /// # Errors
    ///
    /// Returns a typed error if metadata/header evidence changed, the sidecar
    /// is not cross-checked, or another writer is active.
    fn activate_writer(
        &self,
        activation: WriterActivationV1,
    ) -> WorldDbResult<Box<dyn WorldWriter>>;

    /// Verifies an independently retained checkpoint without opening a writer.
    ///
    /// # Errors
    ///
    /// Returns a typed error for missing checkpoints, hash mismatch, malformed
    /// records, or a restore image that is not self-contained.
    fn verify_checkpoint(
        &self,
        world: WorldId,
        checkpoint: CheckpointId,
    ) -> WorldDbResult<CheckpointReceiptV1>;
}

/// Immutable, coherent read snapshot captured at one world revision.
pub trait WorldReadView: Send + Sync {
    /// Returns the world captured by this view.
    #[must_use]
    fn world(&self) -> WorldId;

    /// Returns the persistence frontiers captured with this view.
    #[must_use]
    fn frontier(&self) -> WorldFrontierV1;

    /// Loads one complete chunk from the captured authoritative snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a different world, malformed key or envelope,
    /// integrity failure, or a key/payload identity mismatch.
    fn load_chunk(&self, key: &ChunkKey) -> WorldDbResult<Option<PersistedChunkV1>>;
}

/// Exclusive authoritative writer obtained only after successful preflight.
pub trait WorldWriter: Send {
    /// Returns the world owned by this writer.
    #[must_use]
    fn world(&self) -> WorldId;

    /// Atomically publishes records, entity index, metadata, and receipts.
    ///
    /// `Written` enables the WAL but does not claim media synchronization.
    /// `Durable` uses the backend sync boundary before attempting a recoverable
    /// sidecar publish. A sidecar failure is reported in the outcome because
    /// the authoritative database transaction has already committed.
    ///
    /// # Errors
    ///
    /// Returns a typed pre-publication error for conflicts, limits, malformed
    /// data, writer mismatch, or an injected database failure. It never returns
    /// an ordinary error after authoritative publication.
    fn commit(&mut self, request: WorldCommitRequestV1) -> WorldDbResult<WorldCommitOutcomeV1>;

    /// Synchronizes the complete written frontier and publishes its header.
    ///
    /// # Errors
    ///
    /// Returns a typed error only before the sync metadata batch publishes.
    /// Header failure is represented in the returned outcome.
    fn flush_durable(&mut self) -> WorldDbResult<WorldCommitOutcomeV1>;

    /// Creates and verifies an independent checkpoint at the durable frontier.
    ///
    /// # Errors
    ///
    /// Returns a typed error for a non-durable frontier, duplicate equivalent
    /// checkpoint, limits, writer mismatch, or pre-publication checkpoint fault.
    fn create_checkpoint(
        &mut self,
        request: CheckpointRequestV1,
    ) -> WorldDbResult<CheckpointOutcomeV1>;

    /// Closes the writer lease explicitly.
    ///
    /// # Errors
    ///
    /// Returns a typed error if the lease was already invalidated.
    fn close(self: Box<Self>) -> WorldDbResult<()>;
}
