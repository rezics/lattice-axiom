//! Fail-closed D3 world-persistence boundary and volatile reference.
//!
//! [`DeterministicWorldStorage`] proves only deterministic in-process atomic
//! transitions. It reports [`StorageDurabilityCapabilityV1::VolatileReference`]
//! and rejects durable, flush, and physical-checkpoint operations. Writer
//! activation also remains blocked until the catalog supplies non-forgeable
//! plan evidence that storage can bind and revalidate under its final lock.
//! No filesystem, WAL, restart, or storage-media durability is claimed.
mod contract;
mod error;
mod header;
mod keyspace;
mod memory;
mod model;

pub use contract::{WorldReadView, WorldStorage, WorldWriter};
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
