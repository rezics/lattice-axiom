//! Fail-closed D3 world-persistence boundary, volatile reference, and durable
//! recovery oracle.
//!
//! [`DeterministicWorldStorage::new`] reports
//! [`StorageDurabilityCapabilityV1::VolatileReference`] and rejects durable,
//! flush, physical-checkpoint, and canonical-reopen operations.
//! [`DeterministicWorldStorage::durable`] reports
//! [`StorageDurabilityCapabilityV1::WalSyncCheckpoint`] and proves sealed
//! writer activation, materialized-chunk reads, WAL/sync frontiers, independent
//! checkpoints, lease exclusivity, low-disk admission, read-only recovery, and
//! canonical reopen of the last durable image. Writer activation is authorized
//! only when [`WriterActivationV1`] carries a sealed catalog receipt that
//! matches the storage permit, including plan generation. The storage permit
//! alone is not authority.
mod contract;
mod disk;
mod durable;
mod error;
mod header;
mod indexed;
mod keyspace;
mod memory;
mod model;

pub use contract::{WorldReadView, WorldStorage, WorldWriter};
pub use disk::{DiskWorldEntryV1, DiskWorldError, DiskWorldStore, DurableWorldImageV1};
pub use error::{WorldDbError, WorldDbResult};
pub use header::{
    DeterministicHeaderPublisher, HeaderFaultPointV1, HeaderPublishErrorV1, HeaderPublishReceiptV1,
    HeaderPublishStageV1, HeaderPublisher, PreparedWorldHeaderV1, WorldHeaderBodyV1, WorldHeaderV1,
};
pub use keyspace::{
    ColumnFamilyV1, METADATA_WORLD_PREFIX_LENGTH_V1, MetadataKeyKindV1, MetadataKeyV1,
    decode_metadata_key, encode_metadata_key, metadata_world_prefix,
};
pub use latticeaxiom_world_catalog::{DisplayName, StoreId};
pub use memory::{DatabaseFaultPointV1, DeterministicWorldStorage, FakeKeyspaceStatsV1};
#[cfg(test)]
mod property_tests;

pub use model::{
    ActivationPermitV1, AuthoritativeMetadataInputV1, CheckpointId, CheckpointKindV1,
    CheckpointOutcomeV1, CheckpointReceiptV1, CheckpointRequestV1, ChunkCommitReceiptV1,
    CommitDurabilityV1, CommitHeaderStatusV1, DigestV1, DomainRevisionsV1, FrozenLockReceiptV1,
    FrozenPackageReceiptV1, HeaderRepairPermitV1, HeaderRepairReason, MetadataEpoch,
    PackageRequirementV1, PersistedChunkV1, PreflightInstrumentationV1, RealizationKindV1,
    ReconciliationBlock, SchemaRequirementV1, StorageDurabilityCapabilityV1,
    StoragePreflightStatusV1, WorldCommitOutcomeV1, WorldCommitReceiptV1, WorldCommitRequestV1,
    WorldCreateOutcomeV1, WorldCreateRequestV1, WorldFrontierV1, WorldRequirementClosureV1,
    WorldStorageLimitsV1, WorldStoragePreflightV1, WriterActivationV1,
};
