//! Host wrapper around sealed [`DeterministicWorldStorage`] writer activation.
//!
//! Storage preflight permits are not writer authority. A writer opens only when
//! an accepted [`WorldOpenAction::UseFrozenLock`] plan carries a sealed receipt
//! bound to [`SealedActivationBindingV1`]. This path does not stream voxels
//! through [`super::spine`].

use std::{fmt, sync::Arc};

use latticeaxiom_core::{CanonicalHash, StableId, WorldId};
use latticeaxiom_world_catalog::{
    AcceptedWorldOpenPlan, PlanAcceptanceError, SealedActivationBindingV1, WorldOpenAction,
    WorldOpenPlan,
};
use latticeaxiom_world_db::{
    ActivationPermitV1, DeterministicHeaderPublisher, DeterministicWorldStorage, HeaderPublisher,
    WorldCommitOutcomeV1, WorldCommitRequestV1, WorldCreateOutcomeV1, WorldCreateRequestV1,
    WorldDbError, WorldReadView, WorldStorage, WorldStorageLimitsV1, WorldStoragePreflightV1,
    WorldWriter, WriterActivationV1,
};
use latticeaxiom_world_wire::WorldWireLimits;
use thiserror::Error;

/// Host-owned sealed writer over [`DeterministicWorldStorage`].
pub struct SealedWorldWriterHost {
    storage: DeterministicWorldStorage,
    writer: Option<Box<dyn WorldWriter>>,
}

impl fmt::Debug for SealedWorldWriterHost {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SealedWorldWriterHost")
            .field("storage", &self.storage)
            .field("writer_active", &self.writer.is_some())
            .finish()
    }
}

/// Failure to provision, accept, activate, commit, or close a sealed writer.
#[derive(Debug, Error)]
pub enum SealedWriterHostError {
    /// World-storage contract rejection, including missing activation evidence.
    #[error(transparent)]
    WorldDb(#[from] WorldDbError),
    /// Catalog plan acceptance rejected the requested action.
    #[error(transparent)]
    Plan(#[from] PlanAcceptanceError),
    /// A commit, flush, or close ran without an active writer lease.
    #[error("sealed world writer is not active")]
    WriterInactive,
}

impl SealedWorldWriterHost {
    /// Wraps an existing deterministic world store.
    #[must_use]
    pub fn new(storage: DeterministicWorldStorage) -> Self {
        Self {
            storage,
            writer: None,
        }
    }

    /// Builds a volatile reference store with D3 bootstrap limits.
    #[must_use]
    pub fn volatile_reference(record_owner: StableId, publisher: Arc<dyn HeaderPublisher>) -> Self {
        Self::new(DeterministicWorldStorage::new(
            record_owner,
            WorldWireLimits::default(),
            WorldStorageLimitsV1::D3_BOOTSTRAP,
            publisher,
        ))
    }

    /// Builds a volatile reference store with the in-process header publisher.
    #[must_use]
    pub fn volatile_reference_with_default_publisher(record_owner: StableId) -> Self {
        let publisher: Arc<dyn HeaderPublisher> = Arc::new(DeterministicHeaderPublisher::new());
        Self::volatile_reference(record_owner, publisher)
    }

    /// Returns the wrapped deterministic store.
    #[must_use]
    pub const fn storage(&self) -> &DeterministicWorldStorage {
        &self.storage
    }

    /// Provisions revision-zero authoritative metadata before a writer exists.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error for an existing world or invalid metadata.
    pub fn provision_world(
        &self,
        request: WorldCreateRequestV1,
    ) -> Result<WorldCreateOutcomeV1, SealedWriterHostError> {
        Ok(self.storage.provision_world(request)?)
    }

    /// Performs bounded, read-only storage preflight.
    ///
    /// Preflight does not activate a writer. The returned permit is not
    /// authority to write.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when authoritative metadata cannot be read.
    pub fn preflight(
        &self,
        world: WorldId,
    ) -> Result<WorldStoragePreflightV1, SealedWriterHostError> {
        Ok(self.storage.preflight(world)?)
    }

    /// Accepts one action from an immutable open plan.
    ///
    /// [`WorldOpenAction::UseFrozenLock`] is accepted only when the plan carries
    /// [`WorldOpenPlan::activation_binding`]. A missing binding fails closed
    /// with [`WorldDbError::ActivationEvidenceUnavailable`].
    ///
    /// # Errors
    ///
    /// Returns [`WorldDbError::ActivationEvidenceUnavailable`] when frozen-lock
    /// activation has no bound catalog evidence, or a catalog acceptance error
    /// when the action was not offered.
    pub fn accept(
        &self,
        plan: &WorldOpenPlan,
        action: WorldOpenAction,
    ) -> Result<AcceptedWorldOpenPlan, SealedWriterHostError> {
        if matches!(action, WorldOpenAction::UseFrozenLock) && plan.activation_binding.is_none() {
            return Err(WorldDbError::ActivationEvidenceUnavailable {
                world: plan.world_id,
            }
            .into());
        }
        Ok(plan.accept(action)?)
    }

    /// Opens the unique writer after revalidating sealed catalog evidence.
    ///
    /// The storage permit inside [`WriterActivationV1`] is not sufficient. The
    /// accepted plan must carry a sealed receipt.
    ///
    /// # Errors
    ///
    /// Returns [`WorldDbError::ActivationEvidenceUnavailable`] when the accepted
    /// plan has no receipt, or a typed storage error if the permit is stale or
    /// another writer is active.
    pub fn activate_writer(
        &mut self,
        activation: WriterActivationV1,
    ) -> Result<(), SealedWriterHostError> {
        if self.writer.is_some() {
            return Err(WorldDbError::WriterAlreadyActive {
                world: activation.world(),
            }
            .into());
        }
        self.writer = Some(self.storage.activate_writer(activation)?);
        Ok(())
    }

    /// Publishes [`latticeaxiom_world_db::CommitDurabilityV1::Written`] chunk mutations.
    ///
    /// Durable commits remain [`WorldDbError::PhysicalDurabilityUnsupported`].
    ///
    /// # Errors
    ///
    /// Returns [`SealedWriterHostError::WriterInactive`] when no writer is
    /// leased, or a typed storage error if the commit is rejected.
    pub fn commit(
        &mut self,
        request: WorldCommitRequestV1,
    ) -> Result<WorldCommitOutcomeV1, SealedWriterHostError> {
        self.writer
            .as_mut()
            .ok_or(SealedWriterHostError::WriterInactive)?
            .commit(request)
            .map_err(SealedWriterHostError::from)
    }

    /// Requests a durable frontier flush.
    ///
    /// The volatile reference reports
    /// [`WorldDbError::PhysicalDurabilityUnsupported`].
    ///
    /// # Errors
    ///
    /// Returns [`SealedWriterHostError::WriterInactive`] when no writer is
    /// leased, or the backend durability rejection.
    pub fn flush_durable(&mut self) -> Result<WorldCommitOutcomeV1, SealedWriterHostError> {
        self.writer
            .as_mut()
            .ok_or(SealedWriterHostError::WriterInactive)?
            .flush_durable()
            .map_err(SealedWriterHostError::from)
    }

    /// Returns whether a sealed writer lease is currently open.
    #[must_use]
    pub const fn is_writer_active(&self) -> bool {
        self.writer.is_some()
    }

    /// Closes the writer lease explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`SealedWriterHostError::WriterInactive`] when no writer is
    /// leased, or a typed storage error if the lease was already invalidated.
    pub fn close(&mut self) -> Result<(), SealedWriterHostError> {
        self.writer
            .take()
            .ok_or(SealedWriterHostError::WriterInactive)?
            .close()
            .map_err(SealedWriterHostError::from)
    }

    /// Reopens a writer after [`Self::close`].
    ///
    /// Callers must preflight again and accept [`WorldOpenAction::UseFrozenLock`]
    /// only when a fresh [`SealedActivationBindingV1`] is present. The previous
    /// permit is not reused.
    ///
    /// # Errors
    ///
    /// Returns [`WorldDbError::ActivationEvidenceUnavailable`] when frozen-lock
    /// acceptance has no binding, or a typed storage error if activation fails.
    pub fn reactivate(
        &mut self,
        plan: &WorldOpenPlan,
        permit: ActivationPermitV1,
    ) -> Result<(), SealedWriterHostError> {
        let accepted = self.accept(plan, WorldOpenAction::UseFrozenLock)?;
        self.activate_writer(WriterActivationV1::new(accepted, permit)?)
    }

    /// Captures a bounded, immutable read snapshot.
    ///
    /// # Errors
    ///
    /// Returns a typed storage error when the world does not exist.
    pub fn begin_read(
        &self,
        world: WorldId,
    ) -> Result<Box<dyn WorldReadView>, SealedWriterHostError> {
        Ok(self.storage.begin_read(world)?)
    }
}

/// Bound catalog evidence matching a storage permit.
///
/// The permit is not writer authority. [`WriterActivationV1`] still requires a
/// sealed receipt from an accepted plan that carries this binding.
#[must_use]
pub fn sealed_activation_binding(permit: &ActivationPermitV1) -> SealedActivationBindingV1 {
    SealedActivationBindingV1 {
        store_id: permit.store_id().clone(),
        metadata_epoch: permit.metadata_epoch().get(),
        metadata_hash: CanonicalHash::from_bytes(*permit.metadata_hash().as_bytes()),
        projection_hash: CanonicalHash::from_bytes(*permit.projection_hash().as_bytes()),
        plan_generation: permit.metadata_epoch().get(),
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::{
        collections::{BTreeMap, BTreeSet},
        str::FromStr,
    };

    use latticeaxiom_core::{CanonicalHash, SchemaId, StableId, WorldId};
    use latticeaxiom_storage::{
        ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey, ChunkMutation, ChunkRevision,
        ChunkRevisionExpectation, ContinuationId, DimensionId, PayloadSchemaVersion,
        PersistentEntityId, TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
    };
    use latticeaxiom_world_catalog::{
        ReconciliationState, SealedActivationBindingV1, WorldOpenAction, WorldOpenPlan,
        WorldOpenRisk, WorldOpenStatus,
    };
    use latticeaxiom_world_db::{
        ActivationPermitV1, AuthoritativeMetadataInputV1, CommitDurabilityV1, DisplayName,
        FrozenLockReceiptV1, StoreId, WorldCommitRequestV1, WorldCreateRequestV1, WorldDbError,
        WorldRequirementClosureV1, WriterActivationV1,
    };

    use super::{SealedWorldWriterHost, SealedWriterHostError, sealed_activation_binding};

    fn fixture_host() -> (SealedWorldWriterHost, WorldId, AuthoritativeMetadataInputV1) {
        let world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
            .expect("fixture world UUID is canonical");
        let record_owner = StableId::from_str("latticeaxiom:schema/world-db-chunk@1")
            .expect("fixture record owner is canonical");
        (
            SealedWorldWriterHost::volatile_reference_with_default_publisher(record_owner),
            world,
            fixture_metadata(),
        )
    }

    fn fixture_metadata() -> AuthoritativeMetadataInputV1 {
        let lock = FrozenLockReceiptV1::new(
            br#"{"version":1,"packages":[]}"#.to_vec(),
            BTreeMap::new(),
            fixture_digest(b"registration"),
            fixture_digest(b"semantic"),
            fixture_digest(b"bundles"),
            fixture_digest(b"roles"),
            fixture_digest(b"settings"),
        )
        .expect("fixture frozen-lock metadata is valid");
        let closure = WorldRequirementClosureV1::new(
            BTreeMap::new(),
            BTreeMap::new(),
            BTreeSet::new(),
            BTreeMap::new(),
            fixture_digest(b"bundle-receipts"),
            fixture_digest(b"role-bindings"),
            BTreeMap::new(),
        )
        .expect("fixture requirement closure is valid");
        AuthoritativeMetadataInputV1::new(lock, closure)
    }

    fn fixture_digest(label: &[u8]) -> latticeaxiom_world_db::DigestV1 {
        latticeaxiom_world_db::DigestV1::hash(b"latticeaxiom/world-db-test/v1", label)
    }

    fn fixture_key(world: WorldId) -> ChunkKey {
        ChunkKey::new(
            world,
            DimensionId::from_str("terrenia:dimension/terrenia")
                .expect("fixture dimension is canonical"),
            ChunkCoordinate::new(1, -2, 3),
        )
    }

    fn fixture_data(seed: u8) -> ChunkData {
        let schema = SchemaId::from_str("latticeaxiom:schema/chunk-voxels@1")
            .expect("fixture payload schema is canonical");
        let version =
            PayloadSchemaVersion::new(1).expect("fixture payload schema version is positive");
        let voxels = VersionedPayload::new(schema.clone(), version, vec![seed, seed ^ 0x5a]);
        let entities = BTreeMap::from([(
            PersistentEntityId::from_u128(u128::from(seed)),
            VersionedPayload::new(schema.clone(), version, vec![seed.wrapping_add(1)]),
        )]);
        let continuations = BTreeMap::from([(
            ContinuationId::from_u128(u128::from(seed)),
            VersionedPayload::new(schema, version, vec![seed.wrapping_add(2)]),
        )]);
        let provenance = BTreeMap::from([(
            StableId::from_str("latticeaxiom:provenance/worldgen@1")
                .expect("fixture provenance is canonical"),
            CanonicalHash::digest([seed]),
        )]);
        ChunkData::new(voxels, entities, continuations, provenance)
    }

    fn fixture_written_commit(
        world: WorldId,
        metadata: &AuthoritativeMetadataInputV1,
    ) -> WorldCommitRequestV1 {
        WorldCommitRequestV1::new(
            WorldTransaction::new(
                TransactionId::from_u128(7),
                world,
                WorldRevision::ZERO,
                vec![ChunkMutation::new(
                    fixture_key(world),
                    ChunkRevisionExpectation::Absent,
                    ChangedDomains::ALL,
                    fixture_data(11),
                )],
            ),
            metadata.clone(),
            CommitDurabilityV1::Written,
        )
    }

    fn ready_exact_plan(
        world: WorldId,
        activation_binding: Option<SealedActivationBindingV1>,
    ) -> WorldOpenPlan {
        let action = WorldOpenAction::UseFrozenLock;
        WorldOpenPlan {
            world_id: world,
            status: WorldOpenStatus::ReadyExact,
            risk: WorldOpenRisk::None,
            reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
            next_safe_step: Some(action.clone()),
            actions: vec![action],
            diagnostics: Vec::new(),
            activation_binding,
        }
    }

    fn provision_ready(
        host: &SealedWorldWriterHost,
        world: WorldId,
        metadata: &AuthoritativeMetadataInputV1,
    ) -> ActivationPermitV1 {
        host.provision_world(WorldCreateRequestV1::new(
            world,
            DisplayName::new("Deterministic World").expect("fixture display name is valid"),
            StoreId::new("store-generation-1").expect("fixture store ID is valid"),
            metadata.clone(),
        ))
        .expect("fresh host store provisions");
        host.preflight(world)
            .expect("published header cross-checks authoritative metadata")
            .activation_permit()
            .expect("ready preflight carries one activation permit")
            .clone()
    }

    #[test]
    fn missing_receipt_fails_closed_with_activation_evidence_unavailable() {
        let (mut host, world, metadata) = fixture_host();
        let permit = provision_ready(&host, world, &metadata);
        let plan = ready_exact_plan(world, None);

        assert!(matches!(
            host.accept(&plan, WorldOpenAction::UseFrozenLock),
            Err(SealedWriterHostError::WorldDb(
                WorldDbError::ActivationEvidenceUnavailable { world: found }
            )) if found == world
        ));

        let accepted = plan
            .accept(WorldOpenAction::UseFrozenLock)
            .expect("catalog accept still yields a writable plan without a receipt");
        assert!(accepted.activation_receipt().is_none());
        let activation = WriterActivationV1::new(accepted, permit)
            .expect("writable accept binds to a storage permit");
        assert!(matches!(
            host.activate_writer(activation),
            Err(SealedWriterHostError::WorldDb(
                WorldDbError::ActivationEvidenceUnavailable { world: found }
            )) if found == world
        ));
    }

    #[test]
    fn sealed_commit_then_close_is_visible_to_begin_read_and_can_reactivate() {
        let (mut host, world, metadata) = fixture_host();
        let permit = provision_ready(&host, world, &metadata);
        let plan = ready_exact_plan(world, Some(sealed_activation_binding(&permit)));
        let accepted = host
            .accept(&plan, WorldOpenAction::UseFrozenLock)
            .expect("frozen-lock accept requires bound catalog evidence");
        host.activate_writer(
            WriterActivationV1::new(accepted, permit).expect("sealed accept is writable"),
        )
        .expect("sealed writer activation succeeds");
        host.commit(fixture_written_commit(world, &metadata))
            .expect("sealed writer commits a Written chunk mutation");
        host.close().expect("sealed writer closes");

        let loaded = host
            .begin_read(world)
            .expect("closed world remains readable")
            .load_chunk(&fixture_key(world))
            .expect("portable record decodes")
            .expect("committed chunk exists");
        assert_eq!(loaded.chunk_revision(), ChunkRevision::new(1));

        let reopened = host
            .preflight(world)
            .expect("closing a writer leaves preflight evidence")
            .activation_permit()
            .expect("ready store retains activation evidence")
            .clone();
        let reopen_plan = ready_exact_plan(world, Some(sealed_activation_binding(&reopened)));
        host.reactivate(&reopen_plan, reopened)
            .expect("sealed reactivation succeeds after close");
        host.close().expect("reactivated writer closes");
    }
}
