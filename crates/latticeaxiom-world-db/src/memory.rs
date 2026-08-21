//! Deterministic in-memory implementation of the product storage contract.
//!
//! This backend models atomic database transitions, DB-first header publication,
//! writer leases, portable records, and independent checkpoints. It deliberately
//! makes no filesystem, WAL, restart, or media-durability claim.

use std::{
    collections::{BTreeMap, VecDeque},
    fmt,
    sync::{Arc, Mutex, MutexGuard},
};

use latticeaxiom_core::{CanonicalHash, StableId, WorldId};
use latticeaxiom_storage::{
    ChangedDomains, ChunkData, ChunkKey, ChunkRevision, ChunkRevisionExpectation,
    PersistentEntityId, TransactionId, WorldRevision,
};
use latticeaxiom_world_catalog::{
    AuthoritativeMetadataV1 as CatalogAuthoritativeMetadataV1, CheckpointId as CatalogCheckpointId,
    CheckpointSummary, HeaderObservation, HeaderProjectionV1, LiveWorldLocation, ReadOperation,
    ReconciliationState, SealedWorldActivationReceiptV1, SourceReadError,
    WORLD_HEADER_SCHEMA_VERSION, WorldFingerprints, WorldHeaderV1, WorldRootId, reconcile_header,
};
use latticeaxiom_world_wire::{
    ChunkRecordKey, PersistedChunkDomainRevisionsV1, PersistedChunkSnapshotV1, RecordKind,
    WorldWireLimits, decode_chunk_record_key, decode_persisted_chunk_snapshot_v1,
    encode_persisted_chunk_snapshot_v1,
};
use serde::Serialize;

use crate::keyspace::encode_record_key_for_write;
use crate::model::{next_revision, validate_metadata_limits};
use crate::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, CheckpointId, CheckpointOutcomeV1,
    CheckpointReceiptV1, CheckpointRequestV1, ChunkCommitReceiptV1, CommitDurabilityV1,
    CommitHeaderStatusV1, DigestV1, DisplayName, DomainRevisionsV1, HeaderFaultPointV1,
    HeaderPublishErrorV1, HeaderPublishStageV1, HeaderPublisher, HeaderRepairPermitV1,
    MetadataEpoch, PersistedChunkV1, PreflightInstrumentationV1, PreparedWorldHeaderV1,
    StorageDurabilityCapabilityV1, StoragePreflightStatusV1, StoreId, WorldCommitOutcomeV1,
    WorldCommitReceiptV1, WorldCommitRequestV1, WorldCreateOutcomeV1, WorldCreateRequestV1,
    WorldDbError, WorldDbResult, WorldFrontierV1, WorldReadView, WorldStorage,
    WorldStorageLimitsV1, WorldStoragePreflightV1, WorldWriter, WriterActivationV1,
};

/// One-shot deterministic failures before authoritative fake publication.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DatabaseFaultPointV1 {
    /// Fail after staging but before the atomic database swap.
    BeforeBatchPublication,
    /// Fail after checkpoint copy but before publishing its catalog entry.
    BeforeCheckpointPublication,
}

impl DatabaseFaultPointV1 {
    const fn name(self) -> &'static str {
        match self {
            Self::BeforeBatchPublication => "before batch publication",
            Self::BeforeCheckpointPublication => "before checkpoint publication",
        }
    }
}

/// Bounded evidence about the fake's logical column families.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FakeKeyspaceStatsV1 {
    default_entries: u64,
    metadata_entries: u64,
    record_entries: u64,
    checkpoint_images: u64,
}

impl FakeKeyspaceStatsV1 {
    /// Returns the mandatory-empty default-family count.
    #[must_use]
    pub const fn default_entries(self) -> u64 {
        self.default_entries
    }
    /// Returns the logical versioned metadata-value count.
    #[must_use]
    pub const fn metadata_entries(self) -> u64 {
        self.metadata_entries
    }
    /// Returns the portable chunk-record count.
    #[must_use]
    pub const fn record_entries(self) -> u64 {
        self.record_entries
    }
    /// Returns the independent checkpoint-image count.
    #[must_use]
    pub const fn checkpoint_images(self) -> u64 {
        self.checkpoint_images
    }
}

#[derive(Clone)]
struct RetainedReceipt {
    fingerprint: DigestV1,
    receipt: WorldCommitReceiptV1,
}

#[derive(Clone)]
struct StoredWorld {
    world: WorldId,
    display_name: DisplayName,
    store_id: StoreId,
    metadata_epoch: MetadataEpoch,
    metadata: AuthoritativeMetadataInputV1,
    metadata_hash: DigestV1,
    expected_header_hash: DigestV1,
    frontier: WorldFrontierV1,
    chunks: Arc<BTreeMap<ChunkKey, PersistedChunkV1>>,
    records: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
    entity_index: BTreeMap<PersistentEntityId, ChunkKey>,
    receipts: BTreeMap<TransactionId, RetainedReceipt>,
    receipt_order: VecDeque<TransactionId>,
    checkpoints: BTreeMap<CheckpointId, CheckpointReceiptV1>,
}

#[derive(Clone, Serialize)]
struct CheckpointImage {
    world: WorldId,
    store_id: StoreId,
    source_revision: WorldRevision,
    metadata: AuthoritativeMetadataInputV1,
    records: BTreeMap<Vec<u8>, Vec<u8>>,
}

#[derive(Clone)]
#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
struct RetainedCheckpoint {
    image: CheckpointImage,
    world: WorldId,
    store_id: StoreId,
    receipt: CheckpointReceiptV1,
}

#[derive(Default)]
struct FakeDatabase {
    worlds: BTreeMap<WorldId, StoredWorld>,
    checkpoints: BTreeMap<(WorldId, CheckpointId), RetainedCheckpoint>,
    active_writers: BTreeMap<WorldId, u128>,
    next_lease: u128,
    fault: Option<DatabaseFaultPointV1>,
}

struct StorageInner {
    record_owner: StableId,
    wire_limits: WorldWireLimits,
    limits: WorldStorageLimitsV1,
    publisher: Arc<dyn HeaderPublisher>,
    database: Mutex<FakeDatabase>,
}

/// Complete deterministic storage contract oracle for one world-store root.
#[derive(Clone)]
pub struct DeterministicWorldStorage {
    inner: Arc<StorageInner>,
}

impl fmt::Debug for DeterministicWorldStorage {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeterministicWorldStorage")
            .field("record_owner", &self.inner.record_owner)
            .field("wire_limits", &self.inner.wire_limits)
            .field("limits", &self.inner.limits)
            .finish_non_exhaustive()
    }
}
impl DeterministicWorldStorage {
    /// Creates a fake with an explicit portable record contract and publisher.
    #[must_use]
    pub fn new(
        record_owner: StableId,
        wire_limits: WorldWireLimits,
        limits: WorldStorageLimitsV1,
        publisher: Arc<dyn HeaderPublisher>,
    ) -> Self {
        Self {
            inner: Arc::new(StorageInner {
                record_owner,
                wire_limits,
                limits,
                publisher,
                database: Mutex::new(FakeDatabase::default()),
            }),
        }
    }

    /// Installs one deterministic database fault.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a prior fault remains pending.
    pub fn inject_database_fault_once(&self, point: DatabaseFaultPointV1) -> WorldDbResult<()> {
        let mut database = self.lock("installing database fault")?;
        if database.fault.is_some() {
            return Err(WorldDbError::FaultAlreadyPending);
        }
        database.fault = Some(point);
        Ok(())
    }

    /// Returns bounded logical keyspace counts.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the fake database lock is poisoned.
    pub fn keyspace_stats(&self) -> WorldDbResult<FakeKeyspaceStatsV1> {
        let database = self.lock("reading fake keyspace statistics")?;
        let metadata_entries = usize_to_u64(database.worlds.len(), "world count")?
            .checked_mul(8)
            .ok_or(WorldDbError::LengthOverflow {
                what: "metadata entry count",
            })?;
        let record_entries = database.worlds.values().try_fold(0_u64, |total, world| {
            total
                .checked_add(usize_to_u64(world.records.len(), "record entry count")?)
                .ok_or(WorldDbError::LengthOverflow {
                    what: "record entry count",
                })
        })?;
        Ok(FakeKeyspaceStatsV1 {
            default_entries: 0,
            metadata_entries,
            record_entries,
            checkpoint_images: usize_to_u64(database.checkpoints.len(), "checkpoint image count")?,
        })
    }

    fn lock(&self, operation: &'static str) -> WorldDbResult<MutexGuard<'_, FakeDatabase>> {
        self.inner
            .database
            .lock()
            .map_err(|_| WorldDbError::LockPoisoned { operation })
    }

    fn activate_validated_writer(
        &self,
        permit: &ActivationPermitV1,
    ) -> WorldDbResult<Box<DeterministicWriter>> {
        let (lease, prepared) =
            {
                let mut database = self.lock("activating reference fixture writer")?;
                if database.active_writers.contains_key(&permit.world) {
                    return Err(WorldDbError::WriterAlreadyActive {
                        world: permit.world,
                    });
                }
                let mut staged = database.worlds.get(&permit.world).cloned().ok_or(
                    WorldDbError::WorldNotFound {
                        world: permit.world,
                    },
                )?;
                Self::validate_activation(&staged, permit)?;
                staged.metadata_epoch = staged.metadata_epoch.checked_next()?;
                staged.metadata.mark_writer_open();
                staged.metadata_hash = hash_json(
                    b"latticeaxiom/authoritative-metadata/v1",
                    &staged.metadata,
                    "authoritative metadata v1",
                )?;
                let prepared = self.prepare_header(&staged)?;
                staged.expected_header_hash = prepared.projection_hash();
                database.next_lease =
                    database
                        .next_lease
                        .checked_add(1)
                        .ok_or(WorldDbError::RevisionOverflow {
                            counter: "writer lease identity",
                        })?;
                let lease = database.next_lease;
                database.worlds.insert(permit.world, staged);
                database.active_writers.insert(permit.world, lease);
                (lease, prepared)
            };
        let _header_status = self.publish_header(&prepared);
        Ok(Box::new(DeterministicWriter {
            storage: self.clone(),
            world: permit.world,
            lease,
            closed: false,
        }))
    }

    #[cfg(test)]
    fn activate_reference_fixture(
        &self,
        permit: &ActivationPermitV1,
    ) -> WorldDbResult<Box<DeterministicWriter>> {
        self.activate_validated_writer(permit)
    }

    fn receipt_matches_permit(
        receipt: &SealedWorldActivationReceiptV1,
        permit: &ActivationPermitV1,
    ) -> WorldDbResult<()> {
        receipt
            .verify()
            .map_err(|_| WorldDbError::ActivationEvidenceUnavailable {
                world: permit.world,
            })?;
        if receipt.world_id != permit.world || receipt.store_id != permit.store_id {
            return Err(WorldDbError::ActivationEvidenceUnavailable {
                world: permit.world,
            });
        }
        if receipt.metadata_epoch != permit.metadata_epoch.get() {
            return Err(WorldDbError::ActivationPermitInvalid {
                world: permit.world,
                reason: "metadata epoch mismatch",
            });
        }
        if receipt.metadata_hash != canonical_hash(permit.metadata_hash) {
            return Err(WorldDbError::ActivationPermitHashMismatch {
                world: permit.world,
                field: "metadata_hash",
                permit: permit.metadata_hash,
                actual: DigestV1::from_bytes(*receipt.metadata_hash.as_bytes()),
            });
        }
        if receipt.projection_hash != canonical_hash(permit.projection_hash) {
            return Err(WorldDbError::ActivationPermitHashMismatch {
                world: permit.world,
                field: "projection_hash",
                permit: permit.projection_hash,
                actual: DigestV1::from_bytes(*receipt.projection_hash.as_bytes()),
            });
        }
        Ok(())
    }

    fn prepare_header(&self, world: &StoredWorld) -> WorldDbResult<PreparedWorldHeaderV1> {
        let latest = world
            .checkpoints
            .values()
            .max_by_key(|receipt| (receipt.source_revision(), receipt.id()));
        let latest_id = latest
            .map(|receipt| catalog_checkpoint_id(world.world, receipt.id()))
            .transpose()?;
        let physical_bytes = world
            .checkpoints
            .values()
            .try_fold(0_u64, |total, receipt| {
                total
                    .checked_add(receipt.physical_bytes())
                    .ok_or(WorldDbError::LengthOverflow {
                        what: "checkpoint physical bytes",
                    })
            })?;
        let projection = HeaderProjectionV1 {
            schema_version: WORLD_HEADER_SCHEMA_VERSION,
            world_id: world.world,
            store_id: world.store_id.clone(),
            display_name: world.display_name.clone(),
            metadata_epoch: world.metadata_epoch.get(),
            authoritative_metadata_hash: canonical_hash(world.metadata_hash),
            fingerprints: WorldFingerprints {
                exact_lock: canonical_hash(world.metadata.frozen_lock().lock_hash()),
                registration: canonical_hash(
                    world.metadata.frozen_lock().registration_image_hash(),
                ),
                semantic: canonical_hash(world.metadata.frozen_lock().semantic_image_hash()),
                settings: canonical_hash(
                    world.metadata.frozen_lock().authoritative_settings_hash(),
                ),
            },
            clean_shutdown: world.metadata.clean_shutdown(),
            authoritative_revision: world.frontier.current().get(),
            durable_frontier: world.frontier.durable().get(),
            checkpoints: CheckpointSummary {
                latest: latest_id,
                count: u32::try_from(world.checkpoints.len()).map_err(|_| {
                    WorldDbError::LengthOverflow {
                        what: "checkpoint count",
                    }
                })?,
                latest_revision: latest.map(|receipt| receipt.source_revision().get()),
                physical_bytes,
            },
        };
        let prepared = PreparedWorldHeaderV1::new(projection).map_err(|error| {
            WorldDbError::CorruptMetadata {
                world: world.world,
                section: "catalog header projection",
                reason: error.to_string(),
            }
        })?;
        if prepared.canonical_bytes().len() > self.inner.limits.max_header_bytes() as usize {
            return Err(WorldDbError::CorruptMetadata {
                world: world.world,
                section: "catalog header projection",
                reason: "canonical header exceeds the configured byte limit".to_owned(),
            });
        }
        Ok(prepared)
    }

    fn publish_header(&self, prepared: &PreparedWorldHeaderV1) -> CommitHeaderStatusV1 {
        match self.inner.publisher.publish(prepared) {
            Ok(receipt) => CommitHeaderStatusV1::Published(receipt),
            Err(error) => CommitHeaderStatusV1::RepairRequired {
                stage: header_error_stage(&error),
                reason: error.to_string(),
            },
        }
    }

    fn operation_receipt(
        world: &StoredWorld,
        durability: CommitDurabilityV1,
    ) -> WorldCommitReceiptV1 {
        WorldCommitReceiptV1 {
            transaction_id: None,
            world: world.world,
            world_revision: world.frontier.current(),
            metadata_epoch: world.metadata_epoch,
            frontier: world.frontier,
            chunks: Vec::new(),
            durability,
            replayed: false,
        }
    }

    fn validate_activation(world: &StoredWorld, permit: &ActivationPermitV1) -> WorldDbResult<()> {
        if permit.world != world.world {
            return Err(WorldDbError::ActivationPermitInvalid {
                world: permit.world,
                reason: "world identity mismatch",
            });
        }
        if permit.store_id != world.store_id {
            return Err(WorldDbError::StoreIdentityMismatch {
                world: world.world,
                expected: world.store_id.clone(),
                actual: permit.store_id.clone(),
            });
        }
        if permit.metadata_epoch != world.metadata_epoch {
            return Err(WorldDbError::ActivationPermitStale {
                world: world.world,
                permit_epoch: permit.metadata_epoch,
                actual_epoch: world.metadata_epoch,
            });
        }
        validate_activation_hashes(world, permit)
    }

    fn validate_repair(world: &StoredWorld, permit: &HeaderRepairPermitV1) -> WorldDbResult<()> {
        if permit.world != world.world || permit.store_id != world.store_id {
            return Err(WorldDbError::HeaderRepairPermitInvalid {
                world: permit.world,
                reason: "world or store identity mismatch",
            });
        }
        if permit.metadata_epoch != world.metadata_epoch {
            return Err(WorldDbError::HeaderRepairPermitStale {
                world: world.world,
                permit_epoch: permit.metadata_epoch,
                actual_epoch: world.metadata_epoch,
            });
        }
        if permit.metadata_hash != world.metadata_hash {
            return Err(WorldDbError::HeaderRepairPermitHashMismatch {
                world: world.world,
                field: "authoritative metadata hash",
                permit: permit.metadata_hash,
                actual: world.metadata_hash,
            });
        }
        if permit.projection_hash != world.expected_header_hash {
            return Err(WorldDbError::HeaderRepairPermitHashMismatch {
                world: world.world,
                field: "expected header projection hash",
                permit: permit.projection_hash,
                actual: world.expected_header_hash,
            });
        }
        Ok(())
    }
}

impl WorldStorage for DeterministicWorldStorage {
    fn durability_capability(&self) -> StorageDurabilityCapabilityV1 {
        StorageDurabilityCapabilityV1::VolatileReference
    }

    fn limits(&self) -> WorldStorageLimitsV1 {
        self.inner.limits
    }

    fn provision_world(
        &self,
        request: WorldCreateRequestV1,
    ) -> WorldDbResult<WorldCreateOutcomeV1> {
        validate_metadata_limits(request.metadata(), self.inner.limits)?;
        let metadata_hash = hash_json(
            b"latticeaxiom/authoritative-metadata/v1",
            request.metadata(),
            "authoritative metadata v1",
        )?;
        let mut staged = StoredWorld {
            world: request.world(),
            display_name: request.display_name().clone(),
            store_id: request.store_id().clone(),
            metadata_epoch: MetadataEpoch::new(1),
            metadata: request.metadata().clone(),
            metadata_hash,
            expected_header_hash: DigestV1::default(),
            frontier: WorldFrontierV1::default(),
            chunks: Arc::new(BTreeMap::new()),
            records: Arc::new(BTreeMap::new()),
            entity_index: BTreeMap::new(),
            receipts: BTreeMap::new(),
            receipt_order: VecDeque::new(),
            checkpoints: BTreeMap::new(),
        };
        let prepared = self.prepare_header(&staged)?;
        staged.expected_header_hash = prepared.projection_hash();
        {
            let mut database = self.lock("provisioning world")?;
            if !database.worlds.is_empty() || database.worlds.contains_key(&request.world()) {
                return Err(WorldDbError::WorldAlreadyExists {
                    world: request.world(),
                });
            }
            consume_fault(&mut database, DatabaseFaultPointV1::BeforeBatchPublication)?;
            database.worlds.insert(request.world(), staged.clone());
        }
        let header = self.publish_header(&prepared);
        Ok(WorldCreateOutcomeV1 {
            world: staged.world,
            store_id: staged.store_id,
            metadata_epoch: staged.metadata_epoch,
            header,
        })
    }

    fn preflight(&self, world: WorldId) -> WorldDbResult<WorldStoragePreflightV1> {
        let stored = self
            .lock("reading authoritative preflight metadata")?
            .worlds
            .get(&world)
            .cloned()
            .ok_or(WorldDbError::WorldNotFound { world })?;
        let prepared = self.prepare_header(&stored)?;
        if prepared.projection_hash() != stored.expected_header_hash {
            return Err(WorldDbError::CorruptMetadata {
                world,
                section: "expected header projection hash",
                reason: "stored hash disagrees with authoritative projection".to_owned(),
            });
        }
        let observation = match self
            .inner
            .publisher
            .read_visible(self.inner.limits.max_header_bytes() as usize)
        {
            Ok(None) => HeaderObservation::Missing,
            Ok(Some(bytes)) => match WorldHeaderV1::decode_canonical(&bytes) {
                Ok(header) => HeaderObservation::Present(Box::new(header)),
                Err(error) => HeaderObservation::Invalid(error),
            },
            Err(error) => HeaderObservation::ReadFailed(SourceReadError::Fault {
                operation: ReadOperation::Header,
                code: error.to_string(),
            }),
        };
        let catalog_metadata = CatalogAuthoritativeMetadataV1 {
            projected_header: prepared.header().projection.clone(),
            expected_header_projection_hash: canonical_hash(stored.expected_header_hash),
        };
        let reconciliation = reconcile_header(
            LiveWorldLocation::new(WorldRootId(0), stored.world),
            observation,
            &catalog_metadata,
        );
        let (status, permit, repair_permit) = match reconciliation {
            ReconciliationState::InSync { .. } => (
                StoragePreflightStatusV1::ReadyForActivation,
                Some(ActivationPermitV1 {
                    world: stored.world,
                    store_id: stored.store_id.clone(),
                    metadata_epoch: stored.metadata_epoch,
                    metadata_hash: stored.metadata_hash,
                    projection_hash: stored.expected_header_hash,
                }),
                None,
            ),
            ReconciliationState::RepairRequired { reason, .. } => (
                StoragePreflightStatusV1::HeaderRepairRequired(reason),
                None,
                Some(HeaderRepairPermitV1 {
                    world: stored.world,
                    store_id: stored.store_id.clone(),
                    metadata_epoch: stored.metadata_epoch,
                    metadata_hash: stored.metadata_hash,
                    projection_hash: stored.expected_header_hash,
                }),
            ),
            ReconciliationState::Blocked { reason } => {
                (StoragePreflightStatusV1::Blocked(reason), None, None)
            }
        };
        Ok(WorldStoragePreflightV1 {
            world: stored.world,
            store_id: stored.store_id,
            display_name: stored.display_name,
            metadata_epoch: stored.metadata_epoch,
            metadata: stored.metadata,
            frontier: stored.frontier,
            status,
            permit,
            repair_permit,
            instrumentation: PreflightInstrumentationV1::default(),
        })
    }

    fn begin_read(&self, world: WorldId) -> WorldDbResult<Box<dyn WorldReadView>> {
        let stored = self
            .lock("capturing read snapshot")?
            .worlds
            .get(&world)
            .cloned()
            .ok_or(WorldDbError::WorldNotFound { world })?;
        Ok(Box::new(DeterministicReadView {
            world,
            frontier: stored.frontier,
            records: stored.records,
            record_owner: self.inner.record_owner.clone(),
            wire_limits: self.inner.wire_limits,
        }))
    }

    fn repair_header(&self, permit: HeaderRepairPermitV1) -> WorldDbResult<WorldCommitOutcomeV1> {
        let database = self.lock("revalidating and publishing header repair")?;
        let stored = database
            .worlds
            .get(&permit.world)
            .ok_or(WorldDbError::WorldNotFound {
                world: permit.world,
            })?;
        Self::validate_repair(stored, &permit)?;
        if database.active_writers.contains_key(&permit.world) {
            return Err(WorldDbError::WriterAlreadyActive {
                world: permit.world,
            });
        }
        let stored = stored.clone();
        let prepared = self.prepare_header(&stored)?;
        let header = self.publish_header(&prepared);
        drop(database);
        let durability = if stored.frontier.current() == stored.frontier.durable() {
            CommitDurabilityV1::Durable
        } else {
            CommitDurabilityV1::Written
        };
        Ok(WorldCommitOutcomeV1 {
            receipt: Self::operation_receipt(&stored, durability),
            header,
        })
    }

    fn activate_writer(
        &self,
        activation: WriterActivationV1,
    ) -> WorldDbResult<Box<dyn WorldWriter>> {
        let permit = activation.permit();
        let Some(receipt) = activation.accepted_plan().activation_receipt() else {
            return Err(WorldDbError::ActivationEvidenceUnavailable {
                world: permit.world,
            });
        };
        Self::receipt_matches_permit(receipt, permit)?;
        Ok(self.activate_validated_writer(permit)?)
    }
    fn verify_checkpoint(
        &self,
        _world: WorldId,
        _checkpoint: CheckpointId,
    ) -> WorldDbResult<CheckpointReceiptV1> {
        Err(WorldDbError::PhysicalDurabilityUnsupported {
            operation: "physical checkpoint verification",
        })
    }
}

struct DeterministicReadView {
    world: WorldId,
    frontier: WorldFrontierV1,
    records: Arc<BTreeMap<Vec<u8>, Vec<u8>>>,
    record_owner: StableId,
    wire_limits: WorldWireLimits,
}

impl WorldReadView for DeterministicReadView {
    fn world(&self) -> WorldId {
        self.world
    }

    fn frontier(&self) -> WorldFrontierV1 {
        self.frontier
    }

    fn load_chunk(&self, key: &ChunkKey) -> WorldDbResult<Option<PersistedChunkV1>> {
        if key.world != self.world {
            return Err(WorldDbError::ChunkWorldMismatch {
                transaction_world: self.world,
                key: Box::new(key.clone()),
            });
        }
        decode_record(key, &self.records, &self.record_owner, self.wire_limits)
    }
}

struct DeterministicWriter {
    storage: DeterministicWorldStorage,
    world: WorldId,
    lease: u128,
    closed: bool,
}

impl DeterministicWriter {
    fn validate_lease(&self, database: &FakeDatabase) -> WorldDbResult<()> {
        if self.closed || database.active_writers.get(&self.world) != Some(&self.lease) {
            Err(WorldDbError::WriterLeaseInvalid { world: self.world })
        } else {
            Ok(())
        }
    }

    fn release(&mut self) -> WorldDbResult<()> {
        if self.closed {
            return Err(WorldDbError::WriterLeaseInvalid { world: self.world });
        }
        let prepared = {
            let mut database = self.storage.lock("closing authoritative writer")?;
            self.validate_lease(&database)?;
            let mut staged = database
                .worlds
                .get(&self.world)
                .cloned()
                .ok_or(WorldDbError::WorldNotFound { world: self.world })?;
            staged.metadata_epoch = staged.metadata_epoch.checked_next()?;
            staged.metadata.mark_clean_shutdown();
            staged.metadata_hash = hash_json(
                b"latticeaxiom/authoritative-metadata/v1",
                &staged.metadata,
                "authoritative metadata v1",
            )?;
            let prepared = self.storage.prepare_header(&staged)?;
            staged.expected_header_hash = prepared.projection_hash();
            consume_fault(&mut database, DatabaseFaultPointV1::BeforeBatchPublication)?;
            database.worlds.insert(self.world, staged);
            database.active_writers.remove(&self.world);
            prepared
        };
        let _header_status = self.storage.publish_header(&prepared);
        self.closed = true;
        Ok(())
    }
}

impl WorldWriter for DeterministicWriter {
    fn world(&self) -> WorldId {
        self.world
    }

    fn commit(&mut self, request: WorldCommitRequestV1) -> WorldDbResult<WorldCommitOutcomeV1> {
        commit_world(self, &request)
    }

    fn flush_durable(&mut self) -> WorldDbResult<WorldCommitOutcomeV1> {
        Err(WorldDbError::PhysicalDurabilityUnsupported {
            operation: "durable frontier flush",
        })
    }

    fn create_checkpoint(
        &mut self,
        _request: CheckpointRequestV1,
    ) -> WorldDbResult<CheckpointOutcomeV1> {
        Err(WorldDbError::PhysicalDurabilityUnsupported {
            operation: "physical checkpoint creation",
        })
    }

    fn close(mut self: Box<Self>) -> WorldDbResult<()> {
        self.release()
    }
}

impl Drop for DeterministicWriter {
    fn drop(&mut self) {
        if self.closed {
            return;
        }
        if let Ok(mut database) = self.storage.inner.database.lock()
            && database.active_writers.get(&self.world) == Some(&self.lease)
        {
            database.active_writers.remove(&self.world);
        }
        self.closed = true;
    }
}

fn preflight_commit_request(
    request: &WorldCommitRequestV1,
    limits: WorldStorageLimitsV1,
) -> WorldDbResult<()> {
    let mutations = request.transaction().mutations();
    if mutations.is_empty() {
        return Err(WorldDbError::EmptyTransaction);
    }
    let mutation_count = usize_to_u64(mutations.len(), "transaction mutation count")?;
    if mutation_count > u64::from(limits.max_chunks_per_commit()) {
        return Err(WorldDbError::TransactionChunkLimitExceeded {
            actual: mutation_count,
            maximum: limits.max_chunks_per_commit(),
        });
    }

    let maximum = limits.max_uncompressed_commit_bytes();
    let mut measured = 0_u64;
    for mutation in mutations {
        let data = mutation.data();
        for payload in std::iter::once(data.voxels())
            .chain(data.persistent_entities().values())
            .chain(data.continuations().values())
        {
            let bytes = usize_to_u64(payload.bytes().len(), "versioned payload bytes")?;
            measured = measured
                .checked_add(bytes)
                .ok_or(WorldDbError::LengthOverflow {
                    what: "transaction payload bytes",
                })?;
            if measured > maximum {
                return Err(WorldDbError::TransactionPayloadLimitExceeded {
                    actual: measured,
                    maximum,
                });
            }
        }
    }
    Ok(())
}
fn commit_world(
    writer: &mut DeterministicWriter,
    request: &WorldCommitRequestV1,
) -> WorldDbResult<WorldCommitOutcomeV1> {
    if matches!(request.durability(), CommitDurabilityV1::Durable) {
        return Err(WorldDbError::PhysicalDurabilityUnsupported {
            operation: "durable commit",
        });
    }
    let transaction = request.transaction();
    if transaction.world() != writer.world {
        return Err(WorldDbError::TransactionWorldMismatch {
            writer_world: writer.world,
            transaction_world: transaction.world(),
        });
    }
    validate_metadata_limits(request.metadata(), writer.storage.inner.limits)?;
    preflight_commit_request(request, writer.storage.inner.limits)?;
    let fingerprint = transaction_fingerprint(request)?;
    let mut database = writer
        .storage
        .lock("committing authoritative transaction")?;
    writer.validate_lease(&database)?;
    let current =
        database
            .worlds
            .get(&writer.world)
            .cloned()
            .ok_or(WorldDbError::WorldNotFound {
                world: writer.world,
            })?;
    if let Some(retained) = current.receipts.get(&transaction.id()) {
        if retained.fingerprint != fingerprint {
            return Err(WorldDbError::TransactionIdReuse {
                world: writer.world,
                transaction_id: transaction.id(),
            });
        }
        let mut receipt = retained.receipt.clone();
        receipt.replayed = true;
        let durability = receipt.durability;
        drop(database);
        let header = replay_header(&writer.storage, &current, durability)?;
        return Ok(WorldCommitOutcomeV1 { receipt, header });
    }
    let (mut staged, receipt) = stage_commit(&writer.storage.inner, current, request, fingerprint)?;
    let prepared = writer.storage.prepare_header(&staged)?;
    staged.expected_header_hash = prepared.projection_hash();
    consume_fault(&mut database, DatabaseFaultPointV1::BeforeBatchPublication)?;
    database.worlds.insert(writer.world, staged.clone());
    drop(database);
    let header = if matches!(request.durability(), CommitDurabilityV1::Durable) {
        writer.storage.publish_header(&prepared)
    } else {
        CommitHeaderStatusV1::DeferredUntilDurable
    };
    Ok(WorldCommitOutcomeV1 { receipt, header })
}

#[allow(
    clippy::too_many_lines,
    reason = "atomic staging keeps one auditable publication plan"
)]
fn stage_commit(
    inner: &StorageInner,
    mut staged: StoredWorld,
    request: &WorldCommitRequestV1,
    fingerprint: DigestV1,
) -> WorldDbResult<(StoredWorld, WorldCommitReceiptV1)> {
    let transaction = request.transaction();
    if transaction.mutations().is_empty() {
        return Err(WorldDbError::EmptyTransaction);
    }
    let mutation_count = usize_to_u64(transaction.mutations().len(), "transaction mutation count")?;
    if mutation_count > u64::from(inner.limits.max_chunks_per_commit()) {
        return Err(WorldDbError::TransactionChunkLimitExceeded {
            actual: mutation_count,
            maximum: inner.limits.max_chunks_per_commit(),
        });
    }
    ensure_retry_not_expired(&staged, transaction.id(), transaction.base_world_revision())?;
    if transaction.base_world_revision() != staged.frontier.current() {
        return Err(WorldDbError::WorldRevisionConflict {
            world: transaction.world(),
            expected: transaction.base_world_revision(),
            actual: staged.frontier.current(),
        });
    }

    let mut mutations = transaction.mutations().iter().collect::<Vec<_>>();
    mutations.sort_by(|left, right| left.key().cmp(right.key()));
    let mut previous: Option<&ChunkKey> = None;
    for mutation in &mutations {
        if mutation.key().world != transaction.world() {
            return Err(WorldDbError::ChunkWorldMismatch {
                transaction_world: transaction.world(),
                key: Box::new(mutation.key().clone()),
            });
        }
        if previous == Some(mutation.key()) {
            return Err(WorldDbError::DuplicateChunk {
                key: Box::new(mutation.key().clone()),
            });
        }
        previous = Some(mutation.key());
    }

    if request.metadata().frozen_lock() != staged.metadata.frozen_lock() {
        return Err(WorldDbError::FrozenLockChangeRequiresMigration {
            world: staged.world,
        });
    }
    if !request
        .metadata()
        .requirement_closure()
        .is_monotonic_expansion_of(staged.metadata.requirement_closure())
    {
        return Err(WorldDbError::RequirementClosureRegression {
            world: staged.world,
        });
    }
    let mut next_metadata = request.metadata().clone();
    next_metadata.mark_writer_open();
    let world_revision = WorldRevision::new(next_revision(
        staged.frontier.current().get(),
        "world revision",
    )?);
    let requirement_closure_hash = next_metadata.requirement_closure().content_hash()?;
    let metadata_hash = hash_json(
        b"latticeaxiom/authoritative-metadata/v1",
        &next_metadata,
        "authoritative metadata v1",
    )?;
    let mut chunks = (*staged.chunks).clone();
    let mut records = (*staged.records).clone();
    let mut entity_index = staged.entity_index.clone();

    for mutation in &mutations {
        let Some(current) = chunks.get(mutation.key()) else {
            continue;
        };
        for entity in current.data().persistent_entities().keys() {
            match entity_index.remove(entity) {
                Some(indexed) if indexed == *mutation.key() => {}
                _ => {
                    return Err(WorldDbError::EntityIndexInvariant {
                        world: staged.world,
                        entity: *entity,
                    });
                }
            }
        }
    }

    let mut total_payload_bytes = 0_u64;
    let mut chunk_receipts = Vec::with_capacity(mutations.len());
    for mutation in mutations {
        let current = chunks.get(mutation.key());
        validate_chunk_expectation(current, mutation)?;
        let actual_domains = changed_domains(current.map(PersistedChunkV1::data), mutation.data());
        if actual_domains.is_empty() {
            return Err(WorldDbError::NoopMutation {
                key: Box::new(mutation.key().clone()),
            });
        }
        if actual_domains != mutation.changed_domains() {
            return Err(WorldDbError::ChangedDomainsMismatch {
                key: Box::new(mutation.key().clone()),
                declared: mutation.changed_domains(),
                actual: actual_domains,
            });
        }

        let current_chunk_revision =
            current.map_or(ChunkRevision::ZERO, PersistedChunkV1::chunk_revision);
        let chunk_revision = ChunkRevision::new(next_revision(
            current_chunk_revision.get(),
            "chunk revision",
        )?);
        let domain_revisions = current
            .map_or_else(
                DomainRevisionsV1::default,
                PersistedChunkV1::domain_revisions,
            )
            .advanced(actual_domains)?;
        let persisted = PersistedChunkV1::new(
            mutation.key().clone(),
            world_revision,
            chunk_revision,
            domain_revisions,
            mutation.data().clone(),
            requirement_closure_hash,
        );

        for entity in persisted.data().persistent_entities().keys() {
            if let Some(existing) = entity_index.insert(*entity, mutation.key().clone())
                && existing != *mutation.key()
            {
                return Err(WorldDbError::PersistentEntityCollision {
                    world: staged.world,
                    entity: *entity,
                    existing: Box::new(existing),
                    attempted: Box::new(mutation.key().clone()),
                });
            }
        }

        let (encoded_key, encoded_record, payload_bytes) = encode_record(inner, &persisted)?;
        total_payload_bytes =
            total_payload_bytes
                .checked_add(payload_bytes)
                .ok_or(WorldDbError::LengthOverflow {
                    what: "transaction payload bytes",
                })?;
        if total_payload_bytes > inner.limits.max_uncompressed_commit_bytes() {
            return Err(WorldDbError::TransactionPayloadLimitExceeded {
                actual: total_payload_bytes,
                maximum: inner.limits.max_uncompressed_commit_bytes(),
            });
        }
        records.insert(encoded_key, encoded_record);
        chunks.insert(mutation.key().clone(), persisted);
        chunk_receipts.push(ChunkCommitReceiptV1::new(
            mutation.key().clone(),
            chunk_revision,
            domain_revisions,
            actual_domains,
        ));
    }

    staged.metadata_epoch = staged.metadata_epoch.checked_next()?;
    staged.metadata = next_metadata;
    staged.metadata_hash = metadata_hash;
    staged.frontier = WorldFrontierV1::new(
        world_revision,
        world_revision,
        if matches!(request.durability(), CommitDurabilityV1::Durable) {
            world_revision
        } else {
            staged.frontier.durable()
        },
        staged.frontier.checkpointed(),
    );
    staged.chunks = Arc::new(chunks);
    staged.records = Arc::new(records);
    staged.entity_index = entity_index;

    let receipt = WorldCommitReceiptV1 {
        transaction_id: Some(transaction.id()),
        world: staged.world,
        world_revision,
        metadata_epoch: staged.metadata_epoch,
        frontier: staged.frontier,
        chunks: chunk_receipts,
        durability: request.durability(),
        replayed: false,
    };
    retain_receipt(
        &mut staged,
        transaction.id(),
        fingerprint,
        receipt.clone(),
        inner.limits.max_retained_transaction_receipts(),
    )?;
    Ok((staged, receipt))
}

fn validate_chunk_expectation(
    current: Option<&PersistedChunkV1>,
    mutation: &latticeaxiom_storage::ChunkMutation,
) -> WorldDbResult<()> {
    let actual = current.map(PersistedChunkV1::chunk_revision);
    let matches = match mutation.expected_revision() {
        ChunkRevisionExpectation::Absent => actual.is_none(),
        ChunkRevisionExpectation::Exact(expected) => actual == Some(expected),
    };
    if matches {
        Ok(())
    } else {
        Err(WorldDbError::ChunkRevisionConflict {
            key: Box::new(mutation.key().clone()),
            expected: mutation.expected_revision(),
            actual,
        })
    }
}

fn changed_domains(current: Option<&ChunkData>, replacement: &ChunkData) -> ChangedDomains {
    let Some(current) = current else {
        return ChangedDomains::ALL;
    };
    let mut changed = ChangedDomains::NONE;
    if current.voxels() != replacement.voxels() {
        changed = changed.union(ChangedDomains::VOXELS);
    }
    if current.persistent_entities() != replacement.persistent_entities() {
        changed = changed.union(ChangedDomains::PERSISTENT_ENTITIES);
    }
    if current.continuations() != replacement.continuations() {
        changed = changed.union(ChangedDomains::CONTINUATIONS);
    }
    if current.provenance() != replacement.provenance() {
        changed = changed.union(ChangedDomains::PROVENANCE);
    }
    changed
}

fn encode_record(
    inner: &StorageInner,
    persisted: &PersistedChunkV1,
) -> WorldDbResult<(Vec<u8>, Vec<u8>, u64)> {
    let revisions = persisted.domain_revisions();
    let wire_value = PersistedChunkSnapshotV1::new(
        persisted.key().clone(),
        persisted.captured_world_revision(),
        persisted.chunk_revision(),
        PersistedChunkDomainRevisionsV1::new(
            revisions.voxels(),
            revisions.persistent_entities(),
            revisions.continuation(),
        ),
        persisted.data().clone(),
        *persisted.requirement_closure_hash().as_bytes(),
    );
    let encoded = encode_persisted_chunk_snapshot_v1(&wire_value, inner.wire_limits)?;
    let payload_length = encoded.postcard_payload_bytes();
    let encoded_record = encoded.into_envelope();
    let logical_key = ChunkRecordKey::new(
        persisted.key().clone(),
        RecordKind::CHUNK_SNAPSHOT,
        inner.record_owner.clone(),
    );
    let encoded_key = encode_record_key_for_write(&logical_key, inner.wire_limits)?;
    Ok((encoded_key, encoded_record, payload_length))
}
fn ensure_retry_not_expired(
    world: &StoredWorld,
    transaction_id: TransactionId,
    transaction_base: WorldRevision,
) -> WorldDbResult<()> {
    if transaction_base >= world.frontier.current() {
        return Ok(());
    }
    let Some(oldest_id) = world.receipt_order.front() else {
        return Ok(());
    };
    let oldest = world
        .receipts
        .get(oldest_id)
        .ok_or(WorldDbError::ReceiptHistoryInvariant { world: world.world })?;
    let oldest_replayable_base = oldest
        .receipt
        .world_revision()
        .get()
        .checked_sub(1)
        .map(WorldRevision::new)
        .ok_or(WorldDbError::ReceiptHistoryInvariant { world: world.world })?;
    if transaction_base < oldest_replayable_base {
        return Err(WorldDbError::RetryWindowExpired {
            world: world.world,
            transaction_id,
            transaction_base,
            oldest_replayable_base,
        });
    }
    Ok(())
}

fn retain_receipt(
    world: &mut StoredWorld,
    id: TransactionId,
    fingerprint: DigestV1,
    receipt: WorldCommitReceiptV1,
    maximum: u32,
) -> WorldDbResult<()> {
    world.receipts.insert(
        id,
        RetainedReceipt {
            fingerprint,
            receipt,
        },
    );
    world.receipt_order.push_back(id);
    let maximum = usize::try_from(maximum).map_err(|_| WorldDbError::LengthOverflow {
        what: "retained transaction receipt limit",
    })?;
    while world.receipt_order.len() > maximum {
        let expired = world
            .receipt_order
            .pop_front()
            .ok_or(WorldDbError::ReceiptHistoryInvariant { world: world.world })?;
        if world.receipts.remove(&expired).is_none() {
            return Err(WorldDbError::ReceiptHistoryInvariant { world: world.world });
        }
    }
    Ok(())
}

fn replay_header(
    storage: &DeterministicWorldStorage,
    world: &StoredWorld,
    durability: CommitDurabilityV1,
) -> WorldDbResult<CommitHeaderStatusV1> {
    if matches!(durability, CommitDurabilityV1::Written) {
        return Ok(CommitHeaderStatusV1::DeferredUntilDurable);
    }
    let prepared = storage.prepare_header(world)?;
    Ok(storage.publish_header(&prepared))
}

#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
fn flush_world(writer: &mut DeterministicWriter) -> WorldDbResult<WorldCommitOutcomeV1> {
    let mut database = writer.storage.lock("synchronizing durable frontier")?;
    writer.validate_lease(&database)?;
    let current =
        database
            .worlds
            .get(&writer.world)
            .cloned()
            .ok_or(WorldDbError::WorldNotFound {
                world: writer.world,
            })?;
    validate_frontier(&current)?;
    if current.frontier.current() == current.frontier.durable() {
        let prepared = writer.storage.prepare_header(&current)?;
        let receipt =
            DeterministicWorldStorage::operation_receipt(&current, CommitDurabilityV1::Durable);
        drop(database);
        return Ok(WorldCommitOutcomeV1 {
            receipt,
            header: writer.storage.publish_header(&prepared),
        });
    }

    let mut staged = current;
    staged.metadata_epoch = staged.metadata_epoch.checked_next()?;
    staged.frontier = WorldFrontierV1::new(
        staged.frontier.current(),
        staged.frontier.written(),
        staged.frontier.written(),
        staged.frontier.checkpointed(),
    );
    let prepared = writer.storage.prepare_header(&staged)?;
    staged.expected_header_hash = prepared.projection_hash();
    consume_fault(&mut database, DatabaseFaultPointV1::BeforeBatchPublication)?;
    database.worlds.insert(writer.world, staged.clone());
    let receipt =
        DeterministicWorldStorage::operation_receipt(&staged, CommitDurabilityV1::Durable);
    drop(database);
    Ok(WorldCommitOutcomeV1 {
        receipt,
        header: writer.storage.publish_header(&prepared),
    })
}

#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
fn validate_frontier(world: &StoredWorld) -> WorldDbResult<()> {
    let frontier = world.frontier;
    if frontier.checkpointed() > frontier.durable() {
        return Err(WorldDbError::FrontierInvariant {
            world: world.world,
            reason: "checkpointed frontier exceeds durable frontier",
        });
    }
    if frontier.durable() > frontier.written() {
        return Err(WorldDbError::FrontierInvariant {
            world: world.world,
            reason: "durable frontier exceeds written frontier",
        });
    }
    if frontier.written() > frontier.current() {
        return Err(WorldDbError::FrontierInvariant {
            world: world.world,
            reason: "written frontier exceeds current revision",
        });
    }
    Ok(())
}

#[allow(
    clippy::too_many_lines,
    reason = "checkpoint publication keeps fault ordering explicit"
)]
#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
fn checkpoint_world(
    writer: &mut DeterministicWriter,
    request: &CheckpointRequestV1,
) -> WorldDbResult<CheckpointOutcomeV1> {
    let mut database = writer.storage.lock("creating independent checkpoint")?;
    writer.validate_lease(&database)?;
    let current =
        database
            .worlds
            .get(&writer.world)
            .cloned()
            .ok_or(WorldDbError::WorldNotFound {
                world: writer.world,
            })?;
    validate_frontier(&current)?;
    if current.frontier.current() != current.frontier.durable() {
        return Err(WorldDbError::CheckpointRequiresDurableFrontier {
            world: current.world,
            current: current.frontier.current(),
            durable: current.frontier.durable(),
        });
    }
    if current.checkpoints.contains_key(&request.id())
        || database
            .checkpoints
            .contains_key(&(current.world, request.id()))
    {
        return Err(WorldDbError::CheckpointAlreadyExists {
            world: current.world,
            checkpoint: request.id(),
        });
    }
    let proposed_count = usize_to_u64(current.checkpoints.len(), "checkpoint count")?
        .checked_add(1)
        .ok_or(WorldDbError::LengthOverflow {
            what: "checkpoint count",
        })?;
    if proposed_count > u64::from(writer.storage.inner.limits.max_checkpoints_per_world()) {
        return Err(WorldDbError::CheckpointLimitExceeded {
            world: current.world,
            actual: proposed_count,
            maximum: writer.storage.inner.limits.max_checkpoints_per_world(),
        });
    }
    let exact_lock_hash = current.metadata.frozen_lock().lock_hash();
    if let Some(existing) = current.checkpoints.values().find(|receipt| {
        receipt.source_revision() == current.frontier.current()
            && receipt.exact_lock_hash() == exact_lock_hash
            && receipt.metadata_hash() == current.metadata_hash
    }) {
        return Err(WorldDbError::EquivalentCheckpoint {
            world: current.world,
            requested: request.id(),
            existing: existing.id(),
        });
    }

    let image = CheckpointImage {
        world: current.world,
        store_id: current.store_id.clone(),
        source_revision: current.frontier.current(),
        metadata: current.metadata.clone(),
        records: (*current.records).clone(),
    };
    let image_bytes = postcard::to_allocvec(&image)
        .map_err(|error| WorldDbError::postcard_encode("checkpoint image v1", &error))?;
    let physical_bytes = usize_to_u64(image_bytes.len(), "checkpoint image")?;
    let content_hash = DigestV1::hash(b"latticeaxiom/checkpoint-image/v1", &image_bytes);

    let mut staged = current;
    staged.metadata_epoch = staged.metadata_epoch.checked_next()?;
    staged.frontier = WorldFrontierV1::new(
        staged.frontier.current(),
        staged.frontier.written(),
        staged.frontier.durable(),
        staged.frontier.current(),
    );
    let mut receipt = CheckpointReceiptV1 {
        id: request.id(),
        kind: request.kind(),
        source_revision: staged.frontier.current(),
        exact_lock_hash,
        metadata_hash: staged.metadata_hash,
        header_projection_hash: DigestV1::default(),
        reason: request.reason().to_owned(),
        physical_bytes,
        content_hash,
        restore_verified: true,
    };
    staged.checkpoints.insert(request.id(), receipt.clone());
    let prepared = writer.storage.prepare_header(&staged)?;
    receipt.header_projection_hash = prepared.projection_hash();
    staged.checkpoints.insert(request.id(), receipt.clone());
    staged.expected_header_hash = prepared.projection_hash();
    let prepared_after_receipt = writer.storage.prepare_header(&staged)?;
    if prepared_after_receipt.projection_hash() != prepared.projection_hash() {
        return Err(WorldDbError::CorruptMetadata {
            world: staged.world,
            section: "checkpoint header projection",
            reason: "receipt hash changed the projected catalog summary".to_owned(),
        });
    }

    let retained = RetainedCheckpoint {
        image,
        world: staged.world,
        store_id: staged.store_id.clone(),
        receipt: receipt.clone(),
    };
    verify_checkpoint_image(&writer.storage.inner, &retained)?;
    consume_fault(
        &mut database,
        DatabaseFaultPointV1::BeforeCheckpointPublication,
    )?;
    database.worlds.insert(writer.world, staged);
    database
        .checkpoints
        .insert((writer.world, request.id()), retained);
    drop(database);
    Ok(CheckpointOutcomeV1 {
        receipt,
        header: writer.storage.publish_header(&prepared),
    })
}

#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
fn verify_checkpoint_image(
    inner: &StorageInner,
    retained: &RetainedCheckpoint,
) -> WorldDbResult<()> {
    let receipt = &retained.receipt;
    let image = &retained.image;
    let bytes = postcard::to_allocvec(image)
        .map_err(|error| corrupt_checkpoint(retained, error.to_string()))?;
    let physical_bytes = usize_to_u64(bytes.len(), "checkpoint verification image")
        .map_err(|error| corrupt_checkpoint(retained, error.to_string()))?;
    if physical_bytes != receipt.physical_bytes() {
        return Err(corrupt_checkpoint(
            retained,
            "physical byte count mismatch".to_owned(),
        ));
    }
    let content_hash = DigestV1::hash(b"latticeaxiom/checkpoint-image/v1", &bytes);
    if content_hash != receipt.content_hash() {
        return Err(corrupt_checkpoint(
            retained,
            "checkpoint content hash mismatch".to_owned(),
        ));
    }
    if image.source_revision != receipt.source_revision()
        || image.metadata.frozen_lock().lock_hash() != receipt.exact_lock_hash()
    {
        return Err(corrupt_checkpoint(
            retained,
            "source revision or exact lock receipt mismatch".to_owned(),
        ));
    }
    let metadata_hash = hash_json(
        b"latticeaxiom/authoritative-metadata/v1",
        &image.metadata,
        "checkpoint authoritative metadata v1",
    )
    .map_err(|error| corrupt_checkpoint(retained, error.to_string()))?;
    if metadata_hash != receipt.metadata_hash() {
        return Err(corrupt_checkpoint(
            retained,
            "authoritative metadata hash mismatch".to_owned(),
        ));
    }
    verify_checkpoint_records(inner, retained)
}

#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
fn verify_checkpoint_records(
    inner: &StorageInner,
    retained: &RetainedCheckpoint,
) -> WorldDbResult<()> {
    let image = &retained.image;
    if image.world != retained.world || image.store_id != retained.store_id {
        return Err(corrupt_checkpoint(
            retained,
            "checkpoint world or store generation mismatch".to_owned(),
        ));
    }
    let expected_closure = image
        .metadata
        .requirement_closure()
        .content_hash()
        .map_err(|error| corrupt_checkpoint(retained, error.to_string()))?;
    let mut entity_index = BTreeMap::new();
    for encoded_key in image.records.keys() {
        let logical = decode_chunk_record_key(encoded_key, inner.wire_limits)
            .map_err(|error| corrupt_checkpoint(retained, error.to_string()))?;
        if logical.chunk().world != retained.world
            || logical.record_kind() != RecordKind::CHUNK_SNAPSHOT
            || logical.owner() != &inner.record_owner
        {
            return Err(corrupt_checkpoint(
                retained,
                "checkpoint contains a record outside its closed key contract".to_owned(),
            ));
        }
        let chunk = decode_record(
            logical.chunk(),
            &image.records,
            &inner.record_owner,
            inner.wire_limits,
        )
        .map_err(|error| corrupt_checkpoint(retained, error.to_string()))?
        .ok_or_else(|| {
            corrupt_checkpoint(
                retained,
                "checkpoint record vanished during immutable verification".to_owned(),
            )
        })?;
        if chunk.captured_world_revision() > image.source_revision {
            return Err(corrupt_checkpoint(
                retained,
                "chunk revision was captured after the checkpoint frontier".to_owned(),
            ));
        }
        if chunk.requirement_closure_hash() != expected_closure {
            return Err(corrupt_checkpoint(
                retained,
                "chunk requirement closure disagrees with checkpoint metadata".to_owned(),
            ));
        }
        for entity in chunk.data().persistent_entities().keys() {
            if let Some(existing) = entity_index.insert(*entity, chunk.key().clone())
                && existing != *chunk.key()
            {
                return Err(corrupt_checkpoint(
                    retained,
                    "checkpoint persistent-entity ownership collides".to_owned(),
                ));
            }
        }
    }
    Ok(())
}

fn decode_record(
    key: &ChunkKey,
    records: &BTreeMap<Vec<u8>, Vec<u8>>,
    record_owner: &StableId,
    wire_limits: WorldWireLimits,
) -> WorldDbResult<Option<PersistedChunkV1>> {
    let logical_key = ChunkRecordKey::new(
        key.clone(),
        RecordKind::CHUNK_SNAPSHOT,
        record_owner.clone(),
    );
    let encoded_key = encode_record_key_for_write(&logical_key, wire_limits)?;
    let Some(encoded_record) = records.get(&encoded_key) else {
        return Ok(None);
    };
    let decoded = decode_persisted_chunk_snapshot_v1(encoded_record, wire_limits)
        .map_err(|error| corrupt_chunk(key, error.to_string()))?;
    if decoded.key() != key {
        return Err(corrupt_chunk(
            key,
            "logical record key disagrees with decoded payload key".to_owned(),
        ));
    }
    let revisions = decoded.domain_revisions();
    Ok(Some(PersistedChunkV1::new(
        decoded.key().clone(),
        decoded.captured_world_revision(),
        decoded.chunk_revision(),
        DomainRevisionsV1::from_parts(
            revisions.voxels(),
            revisions.persistent_entities(),
            revisions.continuation(),
        ),
        decoded.data().clone(),
        DigestV1::from_bytes(*decoded.requirement_closure_hash()),
    )))
}
fn corrupt_chunk(key: &ChunkKey, reason: String) -> WorldDbError {
    WorldDbError::CorruptChunkRecord {
        key: Box::new(key.clone()),
        reason,
    }
}

#[allow(
    dead_code,
    reason = "retained reference-checkpoint prototype is not a physical durability capability"
)]
fn corrupt_checkpoint(retained: &RetainedCheckpoint, reason: String) -> WorldDbError {
    WorldDbError::CorruptCheckpoint {
        world: retained.world,
        checkpoint: retained.receipt.id(),
        reason,
    }
}

fn transaction_fingerprint(request: &WorldCommitRequestV1) -> WorldDbResult<DigestV1> {
    let transaction = request.transaction();
    let mut mutations = transaction.mutations().iter().collect::<Vec<_>>();
    mutations.sort_by(|left, right| left.key().cmp(right.key()));
    let bytes = postcard::to_allocvec(&(
        transaction.id(),
        transaction.world(),
        transaction.base_world_revision(),
        mutations,
        request.metadata(),
        request.durability(),
    ))
    .map_err(|error| WorldDbError::postcard_encode("transaction fingerprint v1", &error))?;
    Ok(DigestV1::hash(b"latticeaxiom/world-transaction/v1", &bytes))
}

fn hash_json<T: Serialize>(
    domain: &[u8],
    value: &T,
    artifact: &'static str,
) -> WorldDbResult<DigestV1> {
    let bytes = serde_json::to_vec(value).map_err(|error| WorldDbError::JsonEncode {
        artifact,
        reason: error.to_string(),
    })?;
    Ok(DigestV1::hash(domain, &bytes))
}

fn consume_fault(database: &mut FakeDatabase, expected: DatabaseFaultPointV1) -> WorldDbResult<()> {
    if database.fault == Some(expected) {
        database.fault = None;
        Err(WorldDbError::InjectedDatabaseFault {
            point: expected.name(),
        })
    } else {
        Ok(())
    }
}

fn canonical_hash(digest: DigestV1) -> CanonicalHash {
    CanonicalHash::from_bytes(*digest.as_bytes())
}

fn catalog_checkpoint_id(
    world: WorldId,
    checkpoint: CheckpointId,
) -> WorldDbResult<CatalogCheckpointId> {
    let text = format!("{:032x}", u128::from_be_bytes(*checkpoint.as_bytes()));
    CatalogCheckpointId::new(&text).map_err(|error| WorldDbError::CorruptMetadata {
        world,
        section: "checkpoint catalog identity",
        reason: error.to_string(),
    })
}

fn usize_to_u64(value: usize, what: &'static str) -> WorldDbResult<u64> {
    u64::try_from(value).map_err(|_| WorldDbError::LengthOverflow { what })
}

fn header_error_stage(error: &HeaderPublishErrorV1) -> HeaderPublishStageV1 {
    match error {
        HeaderPublishErrorV1::Injected(HeaderFaultPointV1::TempWrite)
        | HeaderPublishErrorV1::Codec(_)
        | HeaderPublishErrorV1::ReadLimitExceeded { .. }
        | HeaderPublishErrorV1::StatePoisoned => HeaderPublishStageV1::TempWritten,
        HeaderPublishErrorV1::Injected(HeaderFaultPointV1::FileSync) => {
            HeaderPublishStageV1::FileSynced
        }
        HeaderPublishErrorV1::Injected(HeaderFaultPointV1::Replace) => {
            HeaderPublishStageV1::Replaced
        }
        HeaderPublishErrorV1::Injected(HeaderFaultPointV1::DirectorySync) => {
            HeaderPublishStageV1::DirectorySynced
        }
    }
}

fn validate_activation_hashes(
    world: &StoredWorld,
    permit: &ActivationPermitV1,
) -> WorldDbResult<()> {
    if permit.metadata_hash != world.metadata_hash {
        return Err(WorldDbError::ActivationPermitHashMismatch {
            world: world.world,
            field: "authoritative metadata hash",
            permit: permit.metadata_hash,
            actual: world.metadata_hash,
        });
    }
    if permit.projection_hash != world.expected_header_hash {
        return Err(WorldDbError::ActivationPermitHashMismatch {
            world: world.world,
            field: "expected header projection hash",
            permit: permit.projection_hash,
            actual: world.expected_header_hash,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::too_many_lines,
        reason = "deterministic fixtures state every construction and recovery invariant"
    )]

    use std::{
        collections::{BTreeMap, BTreeSet},
        str::FromStr,
        sync::Arc,
    };

    use latticeaxiom_core::{CanonicalHash, SchemaId, StableId, WorldId};
    use latticeaxiom_storage::{
        ChangedDomains, ChunkCoordinate, ChunkData, ChunkKey, ChunkMutation,
        ChunkRevisionExpectation, ContinuationId, DimensionId, PayloadSchemaVersion,
        PersistentEntityId, TransactionId, VersionedPayload, WorldRevision, WorldTransaction,
    };
    use latticeaxiom_world_catalog::{
        ReconciliationState, SealedActivationBindingV1, WorldOpenAction, WorldOpenPlan,
        WorldOpenRisk, WorldOpenStatus,
    };
    use latticeaxiom_world_wire::WorldWireLimits;

    use super::*;
    use crate::{CheckpointKindV1, DeterministicHeaderPublisher};

    fn fixture_digest(label: &[u8]) -> DigestV1 {
        DigestV1::hash(b"latticeaxiom/world-db-test/v1", label)
    }

    fn fixture_metadata() -> AuthoritativeMetadataInputV1 {
        let lock = crate::FrozenLockReceiptV1::new(
            br#"{"version":1,"packages":[]}"#.to_vec(),
            BTreeMap::new(),
            fixture_digest(b"registration"),
            fixture_digest(b"semantic"),
            fixture_digest(b"bundles"),
            fixture_digest(b"roles"),
            fixture_digest(b"settings"),
        )
        .expect("fixture frozen-lock metadata is valid");
        let closure = crate::WorldRequirementClosureV1::new(
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

    fn fixture_metadata_variant(
        lock_bytes: &[u8],
        concrete_content: BTreeSet<StableId>,
    ) -> AuthoritativeMetadataInputV1 {
        let lock = crate::FrozenLockReceiptV1::new(
            lock_bytes.to_vec(),
            BTreeMap::new(),
            fixture_digest(b"registration"),
            fixture_digest(b"semantic"),
            fixture_digest(b"bundles"),
            fixture_digest(b"roles"),
            fixture_digest(b"settings"),
        )
        .expect("fixture frozen-lock metadata is valid");
        let closure = crate::WorldRequirementClosureV1::new(
            BTreeMap::new(),
            BTreeMap::new(),
            concrete_content,
            BTreeMap::new(),
            fixture_digest(b"bundle-receipts"),
            fixture_digest(b"role-bindings"),
            BTreeMap::new(),
        )
        .expect("fixture requirement closure is valid");
        AuthoritativeMetadataInputV1::new(lock, closure)
    }

    fn fixture_storage() -> (
        DeterministicWorldStorage,
        Arc<DeterministicHeaderPublisher>,
        WorldId,
    ) {
        let world = WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")
            .expect("fixture world UUID is canonical");
        let publisher = Arc::new(DeterministicHeaderPublisher::new());
        let erased: Arc<dyn HeaderPublisher> = publisher.clone();
        let storage = DeterministicWorldStorage::new(
            StableId::from_str("latticeaxiom:schema/world-db-chunk@1")
                .expect("fixture record owner is canonical"),
            WorldWireLimits::default(),
            WorldStorageLimitsV1::D3_BOOTSTRAP,
            erased,
        );
        (storage, publisher, world)
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

    fn fixture_request(
        world: WorldId,
        id: u128,
        durability: CommitDurabilityV1,
        metadata: &AuthoritativeMetadataInputV1,
    ) -> WorldCommitRequestV1 {
        WorldCommitRequestV1::new(
            WorldTransaction::new(
                TransactionId::from_u128(id),
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
            durability,
        )
    }

    fn fixture_request_at(
        world: WorldId,
        id: u128,
        base: WorldRevision,
        expected: ChunkRevisionExpectation,
        seed: u8,
        metadata: &AuthoritativeMetadataInputV1,
    ) -> WorldCommitRequestV1 {
        WorldCommitRequestV1::new(
            WorldTransaction::new(
                TransactionId::from_u128(id),
                world,
                base,
                vec![ChunkMutation::new(
                    fixture_key(world),
                    expected,
                    ChangedDomains::ALL,
                    fixture_data(seed),
                )],
            ),
            metadata.clone(),
            CommitDurabilityV1::Written,
        )
    }

    fn fixture_activation(world: WorldId, permit: ActivationPermitV1) -> WriterActivationV1 {
        fixture_activation_with_binding(world, permit, None)
    }

    fn fixture_sealed_activation(world: WorldId, permit: ActivationPermitV1) -> WriterActivationV1 {
        let binding = SealedActivationBindingV1 {
            store_id: permit.store_id.clone(),
            metadata_epoch: permit.metadata_epoch.get(),
            metadata_hash: CanonicalHash::from_bytes(*permit.metadata_hash.as_bytes()),
            projection_hash: CanonicalHash::from_bytes(*permit.projection_hash.as_bytes()),
            plan_generation: permit.metadata_epoch.get(),
        };
        fixture_activation_with_binding(world, permit, Some(binding))
    }

    fn fixture_activation_with_binding(
        world: WorldId,
        permit: ActivationPermitV1,
        activation_binding: Option<SealedActivationBindingV1>,
    ) -> WriterActivationV1 {
        let action = WorldOpenAction::UseFrozenLock;
        let plan = WorldOpenPlan {
            world_id: world,
            status: WorldOpenStatus::ReadyExact,
            risk: WorldOpenRisk::None,
            reconciliation: ReconciliationState::InSync { metadata_epoch: 1 },
            next_safe_step: Some(action.clone()),
            actions: vec![action.clone()],
            diagnostics: Vec::new(),
            activation_binding,
        };
        let accepted = plan
            .accept(action)
            .expect("fixture plan explicitly offers writable activation");
        WriterActivationV1::new(accepted, permit).expect("fixture accepted plan is writable")
    }

    fn provision(
        storage: &DeterministicWorldStorage,
        world: WorldId,
        metadata: &AuthoritativeMetadataInputV1,
    ) -> ActivationPermitV1 {
        let outcome = storage
            .provision_world(WorldCreateRequestV1::new(
                world,
                DisplayName::new("Deterministic World").expect("fixture display name is valid"),
                StoreId::new("store-generation-1").expect("fixture store ID is valid"),
                metadata.clone(),
            ))
            .expect("fresh deterministic store provisions");
        assert_eq!(outcome.metadata_epoch(), MetadataEpoch::new(1));
        assert!(matches!(
            outcome.header(),
            CommitHeaderStatusV1::Published(_)
        ));

        let preflight = storage
            .preflight(world)
            .expect("published header cross-checks authoritative metadata");
        assert_eq!(
            preflight.status(),
            &StoragePreflightStatusV1::ReadyForActivation
        );
        assert_eq!(
            preflight.instrumentation(),
            PreflightInstrumentationV1::default()
        );
        preflight
            .activation_permit()
            .expect("ready preflight carries one activation permit")
            .clone()
    }

    #[test]
    fn preflight_is_pure_and_writer_activation_is_exclusive() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);

        assert!(matches!(
            storage.activate_writer(fixture_activation(world, permit.clone())),
            Err(WorldDbError::ActivationEvidenceUnavailable { world: found }) if found == world
        ));
        let writer = storage
            .activate_writer(fixture_sealed_activation(world, permit.clone()))
            .expect("first sealed writer activation succeeds");
        assert!(matches!(
            storage.activate_reference_fixture(&permit),
            Err(WorldDbError::WriterAlreadyActive { world: found }) if found == world
        ));
        writer
            .close()
            .expect("explicit writer close releases the lease");

        let reopened = storage
            .preflight(world)
            .expect("closing a writer does not invalidate metadata evidence")
            .activation_permit()
            .expect("ready store retains activation evidence")
            .clone();
        storage
            .activate_reference_fixture(&reopened)
            .expect("a closed writer lease can be replaced")
            .close()
            .expect("replacement writer closes cleanly");
    }

    #[test]
    fn sealed_activation_rejects_a_mismatched_metadata_hash() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        let mut binding = SealedActivationBindingV1 {
            store_id: permit.store_id.clone(),
            metadata_epoch: permit.metadata_epoch.get(),
            metadata_hash: CanonicalHash::from_bytes(*permit.metadata_hash.as_bytes()),
            projection_hash: CanonicalHash::from_bytes(*permit.projection_hash.as_bytes()),
            plan_generation: permit.metadata_epoch.get(),
        };
        binding.metadata_hash = CanonicalHash::digest(b"forged-metadata");
        let activation = fixture_activation_with_binding(world, permit.clone(), Some(binding));
        assert!(matches!(
            storage.activate_writer(activation),
            Err(WorldDbError::ActivationPermitHashMismatch {
                world: found,
                field: "metadata_hash",
                ..
            }) if found == world
        ));
    }

    #[test]
    fn sealed_writer_commit_survives_close_and_reactivation() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        let mut writer = storage
            .activate_writer(fixture_sealed_activation(world, permit.clone()))
            .expect("sealed writer activation succeeds");
        writer
            .commit(fixture_request(
                world,
                7,
                CommitDurabilityV1::Written,
                &metadata,
            ))
            .expect("sealed writer commits a chunk");
        writer.close().expect("sealed writer closes");

        let loaded = storage
            .begin_read(world)
            .expect("closed world remains readable")
            .load_chunk(&fixture_key(world))
            .expect("portable record decodes")
            .expect("committed chunk exists");
        assert_eq!(loaded.chunk_revision(), ChunkRevision::new(1));

        let reopened = storage
            .preflight(world)
            .expect("closed writer leaves preflight evidence")
            .activation_permit()
            .expect("ready store retains activation evidence")
            .clone();
        storage
            .activate_writer(fixture_sealed_activation(world, reopened))
            .expect("sealed reactivation succeeds after close")
            .close()
            .expect("reactivated writer closes");
        let reloaded = storage
            .begin_read(world)
            .expect("reactivated world remains readable")
            .load_chunk(&fixture_key(world))
            .expect("portable record still decodes")
            .expect("committed chunk survived reactivation");
        assert_eq!(reloaded.chunk_revision(), ChunkRevision::new(1));
    }

    #[test]
    fn pre_batch_fault_is_atomic_and_exact_retry_is_idempotent() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        let mut writer = storage
            .activate_reference_fixture(&permit)
            .expect("validated writer activation succeeds");
        let request = fixture_request(world, 41, CommitDurabilityV1::Written, &metadata);

        storage
            .inject_database_fault_once(DatabaseFaultPointV1::BeforeBatchPublication)
            .expect("fresh fake accepts one database fault");
        assert!(matches!(
            writer.commit(request.clone()),
            Err(WorldDbError::InjectedDatabaseFault {
                point: "before batch publication"
            })
        ));
        let unchanged = storage
            .begin_read(world)
            .expect("pre-publication fault preserves readable state");
        assert_eq!(unchanged.frontier(), WorldFrontierV1::default());
        assert!(
            unchanged
                .load_chunk(&fixture_key(world))
                .expect("unchanged snapshot remains decodable")
                .is_none()
        );
        assert_eq!(
            storage
                .keyspace_stats()
                .expect("fake statistics remain readable")
                .record_entries(),
            0
        );

        let committed = writer
            .commit(request.clone())
            .expect("same request commits after the one-shot fault");
        assert_eq!(committed.receipt().world_revision(), WorldRevision::new(1));
        assert_eq!(
            committed.receipt().frontier().written(),
            WorldRevision::new(1)
        );
        assert_eq!(
            committed.receipt().frontier().durable(),
            WorldRevision::ZERO
        );
        assert!(matches!(
            committed.header(),
            CommitHeaderStatusV1::DeferredUntilDurable
        ));
        let replay = writer
            .commit(request)
            .expect("exact retained transaction retry returns its receipt");
        assert!(replay.receipt().replayed());
        assert_eq!(replay.receipt().world_revision(), WorldRevision::new(1));
        assert_eq!(
            storage
                .begin_read(world)
                .expect("committed world remains readable")
                .load_chunk(&fixture_key(world))
                .expect("portable record decodes")
                .expect("committed chunk exists")
                .chunk_revision(),
            ChunkRevision::new(1)
        );
        writer.close().expect("writer closes after commit");
    }

    #[test]
    fn volatile_reference_rejects_physical_durability_before_publication() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        assert_eq!(
            storage.durability_capability(),
            StorageDurabilityCapabilityV1::VolatileReference
        );
        let mut writer = storage
            .activate_reference_fixture(&permit)
            .expect("validated writer activation succeeds");
        let before = storage
            .begin_read(world)
            .expect("reference state is readable")
            .frontier();
        assert!(matches!(
            writer.commit(fixture_request(
                world,
                42,
                CommitDurabilityV1::Durable,
                &metadata,
            )),
            Err(WorldDbError::PhysicalDurabilityUnsupported {
                operation: "durable commit"
            })
        ));
        assert_eq!(
            storage
                .begin_read(world)
                .expect("rejected durable request leaves state readable")
                .frontier(),
            before
        );
        writer.close().expect("writer closes cleanly");
    }
    #[test]
    fn volatile_reference_rejects_physical_flush_and_checkpoint_claims() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        let mut writer = storage
            .activate_reference_fixture(&permit)
            .expect("validated writer activation succeeds");
        let checkpoint = CheckpointRequestV1::new(
            CheckpointId::from_u128(1),
            CheckpointKindV1::Protected,
            "not a physical recovery point",
        );
        assert!(matches!(
            writer.flush_durable(),
            Err(WorldDbError::PhysicalDurabilityUnsupported {
                operation: "durable frontier flush"
            })
        ));
        assert!(matches!(
            writer.create_checkpoint(checkpoint.clone()),
            Err(WorldDbError::PhysicalDurabilityUnsupported {
                operation: "physical checkpoint creation"
            })
        ));
        assert!(matches!(
            storage.verify_checkpoint(world, checkpoint.id()),
            Err(WorldDbError::PhysicalDurabilityUnsupported {
                operation: "physical checkpoint verification"
            })
        ));
        assert_eq!(
            storage
                .keyspace_stats()
                .expect("reference statistics remain readable")
                .checkpoint_images(),
            0
        );
        writer.close().expect("writer closes cleanly");
    }
    #[test]
    fn writer_open_drop_and_normal_close_own_the_clean_marker() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        let writer = storage
            .activate_reference_fixture(&permit)
            .expect("reference fixture writer activates");
        assert!(
            !storage
                .preflight(world)
                .expect("open state reads")
                .metadata()
                .clean_shutdown()
        );
        drop(writer);
        let after_drop = storage
            .preflight(world)
            .expect("unclean dropped state reads");
        assert!(!after_drop.metadata().clean_shutdown());
        let permit = after_drop
            .activation_permit()
            .expect("reference storage evidence remains available")
            .clone();
        storage
            .activate_reference_fixture(&permit)
            .expect("reference fixture writer reopens")
            .close()
            .expect("normal close publishes the clean marker");
        assert!(
            storage
                .preflight(world)
                .expect("closed state reads")
                .metadata()
                .clean_shutdown()
        );
    }

    #[test]
    fn catalog_store_identity_mismatch_is_blocked_and_not_repairable() {
        let (storage, publisher, world) = fixture_storage();
        let metadata = fixture_metadata();
        let _permit = provision(&storage, world, &metadata);
        let visible = publisher
            .read_visible(WorldStorageLimitsV1::D3_BOOTSTRAP.max_header_bytes() as usize)
            .expect("fixture header read succeeds")
            .expect("fixture header exists");
        let mut header =
            WorldHeaderV1::decode_canonical(&visible).expect("fixture header is canonical");
        header.projection.store_id = StoreId::new("forged-store-generation")
            .expect("forged fixture store ID is syntactically valid");
        let forged = PreparedWorldHeaderV1::new(header.projection)
            .expect("forged identity header remains structurally valid");
        publisher
            .publish(&forged)
            .expect("fixture publisher accepts forged sidecar bytes");

        let preflight = storage
            .preflight(world)
            .expect("DB metadata remains readable");
        assert!(matches!(
            preflight.status(),
            StoragePreflightStatusV1::Blocked(
                latticeaxiom_world_catalog::ReconciliationBlock::StoreIdentityMismatch { .. }
            )
        ));
        assert!(preflight.activation_permit().is_none());
        assert!(preflight.header_repair_permit().is_none());
    }
    #[test]
    fn closure_expands_but_shrink_and_frozen_lock_change_require_migration() {
        let (storage, _, world) = fixture_storage();
        let base_metadata = fixture_metadata();
        let permit = provision(&storage, world, &base_metadata);
        let mut writer = storage
            .activate_reference_fixture(&permit)
            .expect("reference fixture writer activates");
        let content =
            StableId::from_str("terrenia:block/stone@1").expect("fixture content ID is canonical");
        let expanded = fixture_metadata_variant(
            br#"{"version":1,"packages":[]}"#,
            BTreeSet::from([content.clone()]),
        );
        writer
            .commit(fixture_request_at(
                world,
                60,
                WorldRevision::ZERO,
                ChunkRevisionExpectation::Absent,
                20,
                &expanded,
            ))
            .expect("ordinary commits may monotonically expand the closure");

        assert!(matches!(
            writer.commit(fixture_request_at(
                world,
                61,
                WorldRevision::new(1),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
                21,
                &base_metadata,
            )),
            Err(WorldDbError::RequirementClosureRegression { world: found }) if found == world
        ));
        let changed_lock = fixture_metadata_variant(
            br#"{"version":1,"packages":[],"migration":1}"#,
            BTreeSet::from([content]),
        );
        assert!(matches!(
            writer.commit(fixture_request_at(
                world,
                62,
                WorldRevision::new(1),
                ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
                22,
                &changed_lock,
            )),
            Err(WorldDbError::FrozenLockChangeRequiresMigration { world: found }) if found == world
        ));
        assert_eq!(
            storage
                .begin_read(world)
                .expect("rejected metadata changes leave state readable")
                .frontier()
                .current(),
            WorldRevision::new(1)
        );
        writer.close().expect("writer closes cleanly");
    }
    #[test]
    fn retry_fingerprint_v1_has_frozen_golden_bytes() {
        let (_, _, world) = fixture_storage();
        let request = fixture_request(
            world,
            0x0102_0304,
            CommitDurabilityV1::Written,
            &fixture_metadata(),
        );
        assert_eq!(
            *transaction_fingerprint(&request)
                .expect("fixture fingerprint encoding is canonical")
                .as_bytes(),
            [
                38, 132, 140, 243, 206, 170, 63, 59, 181, 103, 60, 83, 93, 35, 46, 28, 107, 246,
                36, 246, 27, 246, 148, 28, 1, 204, 26, 198, 243, 231, 66, 131
            ]
        );
    }
    #[test]
    fn retry_horizon_rejects_boundary_plus_one_without_reapplying() {
        let (storage, _, world) = fixture_storage();
        let metadata = fixture_metadata();
        let permit = provision(&storage, world, &metadata);
        let mut writer = storage
            .activate_reference_fixture(&permit)
            .expect("reference fixture writer activates");
        let mut first_request = None;
        for revision in 0_u64..65 {
            let request = fixture_request_at(
                world,
                1_000 + u128::from(revision),
                WorldRevision::new(revision),
                if revision == 0 {
                    ChunkRevisionExpectation::Absent
                } else {
                    ChunkRevisionExpectation::Exact(ChunkRevision::new(revision))
                },
                u8::try_from(revision + 30).expect("fixture seed remains in u8"),
                &metadata,
            );
            if revision == 0 {
                first_request = Some(request.clone());
            }
            writer
                .commit(request)
                .expect("fixture horizon commit succeeds");
        }
        let expired = first_request.expect("first request was captured");
        assert!(matches!(
            writer.commit(expired),
            Err(WorldDbError::RetryWindowExpired {
                transaction_base: WorldRevision::ZERO,
                oldest_replayable_base,
                ..
            }) if oldest_replayable_base == WorldRevision::new(1)
        ));
        let at_boundary = fixture_request_at(
            world,
            9_999,
            WorldRevision::new(1),
            ChunkRevisionExpectation::Exact(ChunkRevision::new(1)),
            120,
            &metadata,
        );
        assert!(matches!(
            writer.commit(at_boundary),
            Err(WorldDbError::WorldRevisionConflict {
                expected,
                actual,
                ..
            }) if expected == WorldRevision::new(1) && actual == WorldRevision::new(65)
        ));
        assert_eq!(
            storage
                .begin_read(world)
                .expect("expired retry leaves state readable")
                .frontier()
                .current(),
            WorldRevision::new(65)
        );
        writer.close().expect("writer closes cleanly");
    }
}
