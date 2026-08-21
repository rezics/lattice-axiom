//! Stable product DTOs for the D3 world-storage boundary.

use std::collections::{BTreeMap, BTreeSet};

use latticeaxiom_core::{PackageName, SchemaId, StableId, WorldId};
use latticeaxiom_storage::{
    ChangedDomains, ChunkData, ChunkKey, ChunkRevision, ContinuationRevision,
    PersistentEntityRevision, TransactionId, VoxelRevision, WorldRevision,
};
pub use latticeaxiom_world_catalog::{
    AcceptedWorldOpenPlan, DisplayName, HeaderRepairReason, ReconciliationBlock, StoreId,
};
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{HeaderPublishReceiptV1, HeaderPublishStageV1, WorldDbError, WorldDbResult};

/// Stable SHA-256 digest used by persistence receipts.
#[repr(transparent)]
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct DigestV1([u8; 32]);

impl DigestV1 {
    /// Creates a digest from exact bytes.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Hashes a canonical payload under a stable domain separator.
    #[must_use]
    pub fn hash(domain: &[u8], payload: &[u8]) -> Self {
        let mut hasher = Sha256::new();
        hasher.update(domain);
        hasher.update([0]);
        hasher.update(payload);
        Self(hasher.finalize().into())
    }

    /// Returns exact digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

macro_rules! byte_id {
    ($name:ident, $documentation:literal) => {
        #[doc = $documentation]
        #[repr(transparent)]
        #[derive(
            Clone,
            Copy,
            Debug,
            Default,
            Deserialize,
            Eq,
            Hash,
            Ord,
            PartialEq,
            PartialOrd,
            Serialize,
        )]
        #[serde(transparent)]
        pub struct $name([u8; 16]);

        impl $name {
            /// Creates an identifier from stable network-order bytes.
            #[must_use]
            pub const fn from_bytes(bytes: [u8; 16]) -> Self {
                Self(bytes)
            }

            /// Creates a deterministic fixture identifier.
            #[must_use]
            pub const fn from_u128(value: u128) -> Self {
                Self(value.to_be_bytes())
            }

            /// Returns stable network-order bytes.
            #[must_use]
            pub const fn as_bytes(&self) -> &[u8; 16] {
                &self.0
            }
        }
    };
}

byte_id!(
    CheckpointId,
    "Stable identity of an independently retained checkpoint."
);

/// Checked metadata/header projection generation.
#[repr(transparent)]
#[derive(
    Clone, Copy, Debug, Default, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize,
)]
#[serde(transparent)]
pub struct MetadataEpoch(u64);

impl MetadataEpoch {
    /// Initial epoch before a world is provisioned.
    pub const ZERO: Self = Self(0);

    /// Creates an epoch from its persistent value.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the persistent value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn checked_next(self) -> WorldDbResult<Self> {
        self.0
            .checked_add(1)
            .map(Self)
            .ok_or(WorldDbError::RevisionOverflow {
                counter: "metadata_epoch",
            })
    }
}

/// Realization selected by an exact frozen package lock.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum RealizationKindV1 {
    /// Typed data with no executable native code.
    Data,
    /// Source-built static Rust/Bevy realization.
    NativeStatic,
    /// Portable versioned C-ABI realization.
    PortableNative,
    /// Exact-`EngineBuildId` native realization.
    EngineCoupledNative,
}

/// Exact package/source/artifact selection retained with a frozen lock.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrozenPackageReceiptV1 {
    package: PackageName,
    version: String,
    source_hash: DigestV1,
    artifact_hash: Option<DigestV1>,
    realization: RealizationKindV1,
    abi_requirement: Option<String>,
    engine_build_id: Option<DigestV1>,
}

impl FrozenPackageReceiptV1 {
    /// Validates an exact package receipt.
    ///
    /// # Errors
    ///
    /// Returns a typed error when `version` is not an exact `SemVer` or when an
    /// engine-coupled selection omits its build identity.
    pub fn new(
        package: PackageName,
        version: impl Into<String>,
        source_hash: DigestV1,
        artifact_hash: Option<DigestV1>,
        realization: RealizationKindV1,
        abi_requirement: Option<String>,
        engine_build_id: Option<DigestV1>,
    ) -> WorldDbResult<Self> {
        let version = version.into();
        Version::parse(&version).map_err(|error| WorldDbError::InvalidSemver {
            field: "frozen package version",
            value: version.clone(),
            reason: error.to_string(),
        })?;
        if matches!(realization, RealizationKindV1::EngineCoupledNative)
            && engine_build_id.is_none()
        {
            return Err(WorldDbError::MissingEngineBuildId {
                package: package.to_string(),
            });
        }
        Ok(Self {
            package,
            version,
            source_hash,
            artifact_hash,
            realization,
            abi_requirement,
            engine_build_id,
        })
    }

    /// Returns the logical package identity.
    #[must_use]
    pub const fn package(&self) -> &PackageName {
        &self.package
    }
}

/// Complete exact build/registration receipt retained as authoritative metadata.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct FrozenLockReceiptV1 {
    canonical_lock_bytes: Vec<u8>,
    packages: BTreeMap<PackageName, FrozenPackageReceiptV1>,
    registration_image_hash: DigestV1,
    semantic_image_hash: DigestV1,
    active_bundles_hash: DigestV1,
    role_bindings_hash: DigestV1,
    authoritative_settings_hash: DigestV1,
    lock_hash: DigestV1,
}

impl FrozenLockReceiptV1 {
    /// Builds a content-addressed exact lock receipt.
    ///
    /// # Errors
    ///
    /// Returns a typed error when a package-map key disagrees with the receipt.
    pub fn new(
        canonical_lock_bytes: Vec<u8>,
        packages: BTreeMap<PackageName, FrozenPackageReceiptV1>,
        registration_image_hash: DigestV1,
        semantic_image_hash: DigestV1,
        active_bundles_hash: DigestV1,
        role_bindings_hash: DigestV1,
        authoritative_settings_hash: DigestV1,
    ) -> WorldDbResult<Self> {
        for (package, receipt) in &packages {
            if package != receipt.package() {
                return Err(WorldDbError::PackageReceiptKeyMismatch {
                    key: package.to_string(),
                    receipt: receipt.package().to_string(),
                });
            }
        }
        let lock_hash = DigestV1::hash(b"latticeaxiom/frozen-lock/v1", &canonical_lock_bytes);
        Ok(Self {
            canonical_lock_bytes,
            packages,
            registration_image_hash,
            semantic_image_hash,
            active_bundles_hash,
            role_bindings_hash,
            authoritative_settings_hash,
            lock_hash,
        })
    }

    /// Returns canonical exact lock bytes.
    #[must_use]
    pub fn canonical_lock_bytes(&self) -> &[u8] {
        &self.canonical_lock_bytes
    }

    /// Returns the content address of canonical lock bytes.
    #[must_use]
    pub const fn lock_hash(&self) -> DigestV1 {
        self.lock_hash
    }

    /// Returns the registration image fingerprint.
    #[must_use]
    pub const fn registration_image_hash(&self) -> DigestV1 {
        self.registration_image_hash
    }

    /// Returns the semantic image fingerprint.
    #[must_use]
    pub const fn semantic_image_hash(&self) -> DigestV1 {
        self.semantic_image_hash
    }

    /// Returns the authoritative settings fingerprint.
    #[must_use]
    pub const fn authoritative_settings_hash(&self) -> DigestV1 {
        self.authoritative_settings_hash
    }
}

/// Minimum compatible package requirement derived from persisted data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PackageRequirementV1 {
    package: PackageName,
    compatible_range: String,
    source_release_hash: Option<DigestV1>,
    presentation_optional: bool,
}

impl PackageRequirementV1 {
    /// Validates a minimum package requirement.
    ///
    /// # Errors
    ///
    /// Returns a typed error for malformed `SemVer` range text.
    pub fn new(
        package: PackageName,
        compatible_range: impl Into<String>,
        source_release_hash: Option<DigestV1>,
        presentation_optional: bool,
    ) -> WorldDbResult<Self> {
        let compatible_range = compatible_range.into();
        VersionReq::parse(&compatible_range).map_err(|error| WorldDbError::InvalidSemver {
            field: "world requirement package range",
            value: compatible_range.clone(),
            reason: error.to_string(),
        })?;
        Ok(Self {
            package,
            compatible_range,
            source_release_hash,
            presentation_optional,
        })
    }

    /// Returns the required package.
    #[must_use]
    pub const fn package(&self) -> &PackageName {
        &self.package
    }
}

/// Versioned schema read contract required by persisted bytes.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct SchemaRequirementV1 {
    schema: SchemaId,
    owner: PackageName,
    minimum_readable_version: u32,
    maximum_readable_version: u32,
}

impl SchemaRequirementV1 {
    /// Creates a non-empty inclusive schema read range.
    ///
    /// # Errors
    ///
    /// Returns a typed error for version zero or an inverted range.
    pub fn new(
        schema: SchemaId,
        owner: PackageName,
        minimum_readable_version: u32,
        maximum_readable_version: u32,
    ) -> WorldDbResult<Self> {
        if minimum_readable_version == 0
            || maximum_readable_version == 0
            || minimum_readable_version > maximum_readable_version
        {
            return Err(WorldDbError::InvalidSchemaVersionRange {
                minimum: minimum_readable_version,
                maximum: maximum_readable_version,
            });
        }
        Ok(Self {
            schema,
            owner,
            minimum_readable_version,
            maximum_readable_version,
        })
    }

    /// Returns the schema identity.
    #[must_use]
    pub const fn schema(&self) -> &SchemaId {
        &self.schema
    }
}

/// Minimum closure actually required to preserve persisted authoritative data.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorldRequirementClosureV1 {
    packages: BTreeMap<PackageName, PackageRequirementV1>,
    schemas: BTreeMap<SchemaId, SchemaRequirementV1>,
    concrete_content: BTreeSet<StableId>,
    semantic_contract_majors: BTreeMap<StableId, u32>,
    bundle_receipts_hash: DigestV1,
    role_bindings_hash: DigestV1,
    generator_provenance: BTreeMap<StableId, DigestV1>,
}

impl WorldRequirementClosureV1 {
    /// Validates and creates a canonical requirement closure.
    ///
    /// # Errors
    ///
    /// Returns a typed error for map-key mismatches or version-zero semantic
    /// contract majors.
    pub fn new(
        packages: BTreeMap<PackageName, PackageRequirementV1>,
        schemas: BTreeMap<SchemaId, SchemaRequirementV1>,
        concrete_content: BTreeSet<StableId>,
        semantic_contract_majors: BTreeMap<StableId, u32>,
        bundle_receipts_hash: DigestV1,
        role_bindings_hash: DigestV1,
        generator_provenance: BTreeMap<StableId, DigestV1>,
    ) -> WorldDbResult<Self> {
        for (package, requirement) in &packages {
            if package != requirement.package() {
                return Err(WorldDbError::PackageRequirementKeyMismatch {
                    key: package.to_string(),
                    receipt: requirement.package().to_string(),
                });
            }
        }
        for (schema, requirement) in &schemas {
            if schema != requirement.schema() {
                return Err(WorldDbError::SchemaRequirementKeyMismatch {
                    key: schema.to_string(),
                    receipt: requirement.schema().to_string(),
                });
            }
        }
        if let Some((contract, _)) = semantic_contract_majors
            .iter()
            .find(|(_, major)| **major == 0)
        {
            return Err(WorldDbError::InvalidSemanticContractMajor {
                contract: contract.to_string(),
            });
        }
        Ok(Self {
            packages,
            schemas,
            concrete_content,
            semantic_contract_majors,
            bundle_receipts_hash,
            role_bindings_hash,
            generator_provenance,
        })
    }

    /// Computes the canonical closure content address.
    ///
    /// # Errors
    ///
    /// Returns a typed serialization error if canonical JSON encoding fails.
    pub fn content_hash(&self) -> WorldDbResult<DigestV1> {
        let bytes =
            serde_json::to_vec(self).map_err(|error| WorldDbError::metadata_encode(&error))?;
        Ok(DigestV1::hash(
            b"latticeaxiom/world-requirement-closure/v1",
            &bytes,
        ))
    }

    /// Returns whether `self` is a monotonic expansion of `previous`.
    #[must_use]
    pub fn is_monotonic_expansion_of(&self, previous: &Self) -> bool {
        previous
            .packages
            .iter()
            .all(|(key, value)| self.packages.get(key) == Some(value))
            && previous
                .schemas
                .iter()
                .all(|(key, value)| self.schemas.get(key) == Some(value))
            && previous.concrete_content.is_subset(&self.concrete_content)
            && previous
                .semantic_contract_majors
                .iter()
                .all(|(key, value)| self.semantic_contract_majors.get(key) == Some(value))
            && previous
                .generator_provenance
                .iter()
                .all(|(key, value)| self.generator_provenance.get(key) == Some(value))
            && self.bundle_receipts_hash == previous.bundle_receipts_hash
            && self.role_bindings_hash == previous.role_bindings_hash
    }
}

/// Full authoritative metadata supplied with every world transaction.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct AuthoritativeMetadataInputV1 {
    frozen_lock: FrozenLockReceiptV1,
    requirement_closure: WorldRequirementClosureV1,
    clean_shutdown: bool,
}

impl AuthoritativeMetadataInputV1 {
    /// Creates authoritative metadata input.
    #[must_use]
    pub const fn new(
        frozen_lock: FrozenLockReceiptV1,
        requirement_closure: WorldRequirementClosureV1,
    ) -> Self {
        Self {
            frozen_lock,
            requirement_closure,
            clean_shutdown: true,
        }
    }

    /// Returns the exact frozen lock receipt.
    #[must_use]
    pub const fn frozen_lock(&self) -> &FrozenLockReceiptV1 {
        &self.frozen_lock
    }

    /// Returns the minimum persisted-data closure.
    #[must_use]
    pub const fn requirement_closure(&self) -> &WorldRequirementClosureV1 {
        &self.requirement_closure
    }

    /// Returns the clean-shutdown marker projected to the catalog header.
    #[must_use]
    pub const fn clean_shutdown(&self) -> bool {
        self.clean_shutdown
    }

    pub(crate) fn mark_writer_open(&mut self) {
        self.clean_shutdown = false;
    }

    pub(crate) fn mark_clean_shutdown(&mut self) {
        self.clean_shutdown = true;
    }
}

/// Per-domain revisions used only for derived-work invalidation.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DomainRevisionsV1 {
    voxels: VoxelRevision,
    persistent_entities: PersistentEntityRevision,
    continuation: ContinuationRevision,
}

impl DomainRevisionsV1 {
    pub(crate) const fn from_parts(
        voxels: VoxelRevision,
        persistent_entities: PersistentEntityRevision,
        continuation: ContinuationRevision,
    ) -> Self {
        Self {
            voxels,
            persistent_entities,
            continuation,
        }
    }

    /// Returns the voxel revision.
    #[must_use]
    pub const fn voxels(self) -> VoxelRevision {
        self.voxels
    }

    /// Returns the persistent-entity revision.
    #[must_use]
    pub const fn persistent_entities(self) -> PersistentEntityRevision {
        self.persistent_entities
    }

    /// Returns the simulation-continuation revision.
    #[must_use]
    pub const fn continuation(self) -> ContinuationRevision {
        self.continuation
    }

    pub(crate) fn advanced(self, changed: ChangedDomains) -> WorldDbResult<Self> {
        Ok(Self {
            voxels: if changed.contains(ChangedDomains::VOXELS) {
                VoxelRevision::new(next_revision(self.voxels.get(), "voxel revision")?)
            } else {
                self.voxels
            },
            persistent_entities: if changed.contains(ChangedDomains::PERSISTENT_ENTITIES) {
                PersistentEntityRevision::new(next_revision(
                    self.persistent_entities.get(),
                    "persistent-entity revision",
                )?)
            } else {
                self.persistent_entities
            },
            continuation: if changed.contains(ChangedDomains::CONTINUATIONS) {
                ContinuationRevision::new(next_revision(
                    self.continuation.get(),
                    "continuation revision",
                )?)
            } else {
                self.continuation
            },
        })
    }
}

/// Complete decoded authoritative chunk record.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PersistedChunkV1 {
    key: ChunkKey,
    captured_world_revision: WorldRevision,
    chunk_revision: ChunkRevision,
    domain_revisions: DomainRevisionsV1,
    data: ChunkData,
    requirement_closure_hash: DigestV1,
}

impl PersistedChunkV1 {
    pub(crate) const fn new(
        key: ChunkKey,
        captured_world_revision: WorldRevision,
        chunk_revision: ChunkRevision,
        domain_revisions: DomainRevisionsV1,
        data: ChunkData,
        requirement_closure_hash: DigestV1,
    ) -> Self {
        Self {
            key,
            captured_world_revision,
            chunk_revision,
            domain_revisions,
            data,
            requirement_closure_hash,
        }
    }

    /// Returns the complete chunk key.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the transaction world revision captured by this record.
    #[must_use]
    pub const fn captured_world_revision(&self) -> WorldRevision {
        self.captured_world_revision
    }

    /// Returns the complete authoritative chunk revision.
    #[must_use]
    pub const fn chunk_revision(&self) -> ChunkRevision {
        self.chunk_revision
    }

    /// Returns derived invalidation revisions.
    #[must_use]
    pub const fn domain_revisions(&self) -> DomainRevisionsV1 {
        self.domain_revisions
    }

    /// Returns complete authoritative chunk data.
    #[must_use]
    pub const fn data(&self) -> &ChunkData {
        &self.data
    }

    /// Returns the persisted-data requirement closure captured by this record.
    #[must_use]
    pub const fn requirement_closure_hash(&self) -> DigestV1 {
        self.requirement_closure_hash
    }
}

/// Physical persistence boundary implemented by a storage backend.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StorageDurabilityCapabilityV1 {
    /// Volatile deterministic reference only; no WAL, sync, or crash-reopen claim.
    VolatileReference,
    /// Physical WAL writes, sync frontiers, and independently restorable checkpoints.
    WalSyncCheckpoint,
}

/// Synchronous durability requested for one product commit.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CommitDurabilityV1 {
    /// WAL-enabled write accepted without a media-sync claim.
    Written,
    /// WAL-enabled write accepted with the backend sync boundary.
    Durable,
}

/// Atomic world transaction plus its complete authoritative metadata state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorldCommitRequestV1 {
    transaction: latticeaxiom_storage::WorldTransaction,
    metadata: AuthoritativeMetadataInputV1,
    durability: CommitDurabilityV1,
}

impl WorldCommitRequestV1 {
    /// Creates a product transaction.
    #[must_use]
    pub const fn new(
        transaction: latticeaxiom_storage::WorldTransaction,
        metadata: AuthoritativeMetadataInputV1,
        durability: CommitDurabilityV1,
    ) -> Self {
        Self {
            transaction,
            metadata,
            durability,
        }
    }

    /// Returns the chunk transaction.
    #[must_use]
    pub const fn transaction(&self) -> &latticeaxiom_storage::WorldTransaction {
        &self.transaction
    }

    /// Returns complete authoritative metadata input.
    #[must_use]
    pub const fn metadata(&self) -> &AuthoritativeMetadataInputV1 {
        &self.metadata
    }

    /// Returns the requested durability boundary.
    #[must_use]
    pub const fn durability(&self) -> CommitDurabilityV1 {
        self.durability
    }
}

/// Contiguous authoritative persistence frontiers for one world.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct WorldFrontierV1 {
    current: WorldRevision,
    written: WorldRevision,
    durable: WorldRevision,
    checkpointed: WorldRevision,
}

impl WorldFrontierV1 {
    pub(crate) const fn new(
        current: WorldRevision,
        written: WorldRevision,
        durable: WorldRevision,
        checkpointed: WorldRevision,
    ) -> Self {
        Self {
            current,
            written,
            durable,
            checkpointed,
        }
    }

    /// Returns the latest authoritative world revision.
    #[must_use]
    pub const fn current(self) -> WorldRevision {
        self.current
    }

    /// Returns the contiguous WAL-written frontier.
    #[must_use]
    pub const fn written(self) -> WorldRevision {
        self.written
    }

    /// Returns the contiguous media-synchronized frontier.
    #[must_use]
    pub const fn durable(self) -> WorldRevision {
        self.durable
    }

    /// Returns the latest independently verified checkpoint frontier.
    #[must_use]
    pub const fn checkpointed(self) -> WorldRevision {
        self.checkpointed
    }
}

/// Hard resource ceilings owned by a `WorldStorage` instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[allow(
    clippy::struct_field_names,
    reason = "public getters retain explicit maximum semantics"
)]
pub struct WorldStorageLimitsV1 {
    max_chunks_per_commit: u32,
    max_uncompressed_commit_bytes: u64,
    max_automatic_retry_attempts: u8,
    max_retained_transaction_receipts: u32,
    max_header_bytes: u32,
    max_lock_bytes: u32,
    max_requirement_entries: u32,
    max_checkpoints_per_world: u32,
}

impl WorldStorageLimitsV1 {
    /// ADR-0027 bootstrap safety ceilings for the D3 contract oracle.
    pub const D3_BOOTSTRAP: Self = Self {
        max_chunks_per_commit: 32,
        max_uncompressed_commit_bytes: 64 * 1024 * 1024,
        max_automatic_retry_attempts: 3,
        max_retained_transaction_receipts: 64,
        max_header_bytes: 64 * 1024,
        max_lock_bytes: 4 * 1024 * 1024,
        max_requirement_entries: 65_536,
        max_checkpoints_per_world: 64,
    };

    /// Returns the chunk-count ceiling per atomic transaction.
    #[must_use]
    pub const fn max_chunks_per_commit(self) -> u32 {
        self.max_chunks_per_commit
    }

    /// Returns the canonical uncompressed byte ceiling per transaction.
    #[must_use]
    pub const fn max_uncompressed_commit_bytes(self) -> u64 {
        self.max_uncompressed_commit_bytes
    }

    /// Returns the bounded automatic retry count.
    #[must_use]
    pub const fn max_automatic_retry_attempts(self) -> u8 {
        self.max_automatic_retry_attempts
    }

    /// Returns the bounded exact-replay receipt horizon.
    #[must_use]
    pub const fn max_retained_transaction_receipts(self) -> u32 {
        self.max_retained_transaction_receipts
    }

    /// Returns the canonical header byte ceiling.
    #[must_use]
    pub const fn max_header_bytes(self) -> u32 {
        self.max_header_bytes
    }

    /// Returns the exact lock byte ceiling.
    #[must_use]
    pub const fn max_lock_bytes(self) -> u32 {
        self.max_lock_bytes
    }

    /// Returns the combined typed requirement-entry ceiling.
    #[must_use]
    pub const fn max_requirement_entries(self) -> u32 {
        self.max_requirement_entries
    }

    /// Returns the retained checkpoint-count ceiling.
    #[must_use]
    pub const fn max_checkpoints_per_world(self) -> u32 {
        self.max_checkpoints_per_world
    }
}

impl Default for WorldStorageLimitsV1 {
    fn default() -> Self {
        Self::D3_BOOTSTRAP
    }
}

/// Immutable world provisioning request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldCreateRequestV1 {
    world: WorldId,
    display_name: DisplayName,
    store_id: StoreId,
    metadata: AuthoritativeMetadataInputV1,
}

impl WorldCreateRequestV1 {
    /// Creates a new world-store generation at revision zero.
    #[must_use]
    pub const fn new(
        world: WorldId,
        display_name: DisplayName,
        store_id: StoreId,
        metadata: AuthoritativeMetadataInputV1,
    ) -> Self {
        Self {
            world,
            display_name,
            store_id,
            metadata,
        }
    }

    /// Returns the world identity.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the canonical user-facing world name.
    #[must_use]
    pub const fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    /// Returns the immutable physical store-generation identity.
    #[must_use]
    pub const fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    /// Returns the revision-zero authoritative metadata.
    #[must_use]
    pub const fn metadata(&self) -> &AuthoritativeMetadataInputV1 {
        &self.metadata
    }
}

/// Recoverable result of DB-first world provisioning.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldCreateOutcomeV1 {
    pub(crate) world: WorldId,
    pub(crate) store_id: StoreId,
    pub(crate) metadata_epoch: MetadataEpoch,
    pub(crate) header: CommitHeaderStatusV1,
}

impl WorldCreateOutcomeV1 {
    /// Returns the new world identity.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the immutable physical store-generation identity.
    #[must_use]
    pub const fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    /// Returns the authoritative metadata epoch committed during provisioning.
    #[must_use]
    pub const fn metadata_epoch(&self) -> MetadataEpoch {
        self.metadata_epoch
    }

    /// Returns the header publication status after authoritative DB creation.
    #[must_use]
    pub const fn header(&self) -> &CommitHeaderStatusV1 {
        &self.header
    }
}

/// Storage-only result of catalog-owned DB-first reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StoragePreflightStatusV1 {
    /// Header and authoritative DB metadata match and activation may proceed.
    ReadyForActivation,
    /// DB metadata is valid but the bounded sidecar needs explicit repair.
    HeaderRepairRequired(HeaderRepairReason),
    /// Identity or authoritative-metadata integrity blocks repair and open.
    Blocked(ReconciliationBlock),
}
/// Proof that pure storage preflight opened no runtime or writer capability.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PreflightInstrumentationV1 {
    /// Writer activations performed during preflight; always zero.
    pub writer_activations: u32,
    /// Module callbacks performed during preflight; always zero.
    pub module_callbacks: u32,
    /// Bevy worlds created during preflight; always zero.
    pub bevy_worlds_created: u32,
    /// Authoritative commands applied during preflight; always zero.
    pub authoritative_commands: u32,
}

/// Immutable evidence revalidated immediately before writer activation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivationPermitV1 {
    pub(crate) world: WorldId,
    pub(crate) store_id: StoreId,
    pub(crate) metadata_epoch: MetadataEpoch,
    pub(crate) metadata_hash: DigestV1,
    pub(crate) projection_hash: DigestV1,
}

impl ActivationPermitV1 {
    /// Returns the world named by this permit.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the immutable physical store-generation identity.
    #[must_use]
    pub const fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    /// Returns the metadata epoch captured by preflight.
    #[must_use]
    pub const fn metadata_epoch(&self) -> MetadataEpoch {
        self.metadata_epoch
    }

    /// Returns the authoritative metadata hash captured by preflight.
    #[must_use]
    pub const fn metadata_hash(&self) -> DigestV1 {
        self.metadata_hash
    }

    /// Returns the header projection hash captured by preflight.
    #[must_use]
    pub const fn projection_hash(&self) -> DigestV1 {
        self.projection_hash
    }
}

/// Catalog acceptance and storage evidence consumed together at activation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WriterActivationV1 {
    accepted_plan: AcceptedWorldOpenPlan,
    permit: ActivationPermitV1,
}

impl WriterActivationV1 {
    /// Binds an explicitly accepted writable catalog plan to storage evidence.
    ///
    /// The storage permit is not writer authority by itself. Activation still
    /// requires a sealed receipt on the accepted plan.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the accepted action cannot open a writer.
    pub fn new(
        accepted_plan: AcceptedWorldOpenPlan,
        permit: ActivationPermitV1,
    ) -> WorldDbResult<Self> {
        if !accepted_plan.writable() {
            return Err(WorldDbError::ActivationPermitInvalid {
                world: permit.world,
                reason: "accepted catalog plan is not writable",
            });
        }
        Ok(Self {
            accepted_plan,
            permit,
        })
    }

    /// Returns the world named by the bound storage permit.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.permit.world
    }

    /// Returns the storage permit bound to this activation.
    ///
    /// The permit is not writer authority without the accepted plan's sealed
    /// receipt.
    #[must_use]
    pub const fn permit(&self) -> &ActivationPermitV1 {
        &self.permit
    }

    /// Returns the accepted catalog plan bound to this activation.
    #[must_use]
    pub const fn accepted_plan(&self) -> &AcceptedWorldOpenPlan {
        &self.accepted_plan
    }
}

/// Immutable DB evidence authorizing pre-writer sidecar repair.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderRepairPermitV1 {
    pub(crate) world: WorldId,
    pub(crate) store_id: StoreId,
    pub(crate) metadata_epoch: MetadataEpoch,
    pub(crate) metadata_hash: DigestV1,
    pub(crate) projection_hash: DigestV1,
}

/// Result of pure storage preflight.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldStoragePreflightV1 {
    pub(crate) world: WorldId,
    pub(crate) store_id: StoreId,
    pub(crate) display_name: DisplayName,
    pub(crate) metadata_epoch: MetadataEpoch,
    pub(crate) metadata: AuthoritativeMetadataInputV1,
    pub(crate) frontier: WorldFrontierV1,
    pub(crate) status: StoragePreflightStatusV1,
    pub(crate) permit: Option<ActivationPermitV1>,
    pub(crate) repair_permit: Option<HeaderRepairPermitV1>,
    pub(crate) instrumentation: PreflightInstrumentationV1,
}

impl WorldStoragePreflightV1 {
    /// Returns the authoritative world identity.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the immutable physical store-generation identity.
    #[must_use]
    pub const fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    /// Returns the canonical user-facing world name from authoritative metadata.
    #[must_use]
    pub const fn display_name(&self) -> &DisplayName {
        &self.display_name
    }

    /// Returns the authoritative metadata projection epoch.
    #[must_use]
    pub const fn metadata_epoch(&self) -> MetadataEpoch {
        self.metadata_epoch
    }

    /// Returns the immutable DB-derived compatibility-planning metadata.
    #[must_use]
    pub const fn metadata(&self) -> &AuthoritativeMetadataInputV1 {
        &self.metadata
    }

    /// Returns the contiguous persistence frontiers observed by preflight.
    #[must_use]
    pub const fn frontier(&self) -> WorldFrontierV1 {
        self.frontier
    }

    /// Returns the storage readiness status.
    #[must_use]
    pub const fn status(&self) -> &StoragePreflightStatusV1 {
        &self.status
    }

    /// Returns an activation permit only for a cross-checked header/DB pair.
    #[must_use]
    pub const fn activation_permit(&self) -> Option<&ActivationPermitV1> {
        self.permit.as_ref()
    }

    /// Returns a repair permit only when authoritative DB metadata is valid
    /// but its bounded sidecar is missing, stale, or corrupt.
    #[must_use]
    pub const fn header_repair_permit(&self) -> Option<&HeaderRepairPermitV1> {
        self.repair_permit.as_ref()
    }

    /// Returns zero-side-effect preflight instrumentation.
    #[must_use]
    pub const fn instrumentation(&self) -> PreflightInstrumentationV1 {
        self.instrumentation
    }
}

/// Per-chunk details of an authoritative product commit.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ChunkCommitReceiptV1 {
    key: ChunkKey,
    chunk_revision: ChunkRevision,
    domain_revisions: DomainRevisionsV1,
    changed_domains: ChangedDomains,
}

impl ChunkCommitReceiptV1 {
    pub(crate) const fn new(
        key: ChunkKey,
        chunk_revision: ChunkRevision,
        domain_revisions: DomainRevisionsV1,
        changed_domains: ChangedDomains,
    ) -> Self {
        Self {
            key,
            chunk_revision,
            domain_revisions,
            changed_domains,
        }
    }

    /// Returns the committed chunk key.
    #[must_use]
    pub const fn key(&self) -> &ChunkKey {
        &self.key
    }

    /// Returns the committed complete-chunk revision.
    #[must_use]
    pub const fn chunk_revision(&self) -> ChunkRevision {
        self.chunk_revision
    }

    /// Returns the derived invalidation revisions after the commit.
    #[must_use]
    pub const fn domain_revisions(&self) -> DomainRevisionsV1 {
        self.domain_revisions
    }

    /// Returns the authoritative domains changed by the commit.
    #[must_use]
    pub const fn changed_domains(&self) -> ChangedDomains {
        self.changed_domains
    }
}

/// Sidecar result after an authoritative database publication.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CommitHeaderStatusV1 {
    /// A non-sync `Written` commit intentionally left the durable header alone.
    DeferredUntilDurable,
    /// The bounded header completed temp-write, file sync, replace, and directory sync.
    Published(HeaderPublishReceiptV1),
    /// The DB is authoritative and ahead; explicit header reconciliation is required.
    RepairRequired {
        /// Last publisher stage reached or attempted.
        stage: HeaderPublishStageV1,
        /// Stable diagnostic from the publisher.
        reason: String,
    },
}

/// Receipt for a transaction, durable flush, or metadata reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldCommitReceiptV1 {
    pub(crate) transaction_id: Option<TransactionId>,
    pub(crate) world: WorldId,
    pub(crate) world_revision: WorldRevision,
    pub(crate) metadata_epoch: MetadataEpoch,
    pub(crate) frontier: WorldFrontierV1,
    pub(crate) chunks: Vec<ChunkCommitReceiptV1>,
    pub(crate) durability: CommitDurabilityV1,
    pub(crate) replayed: bool,
}

impl WorldCommitReceiptV1 {
    /// Returns the idempotency identity for a transaction receipt.
    #[must_use]
    pub const fn transaction_id(&self) -> Option<TransactionId> {
        self.transaction_id
    }

    /// Returns the committed world identity.
    #[must_use]
    pub const fn world(&self) -> WorldId {
        self.world
    }

    /// Returns the authoritative world revision.
    #[must_use]
    pub const fn world_revision(&self) -> WorldRevision {
        self.world_revision
    }

    /// Returns the authoritative metadata projection epoch.
    #[must_use]
    pub const fn metadata_epoch(&self) -> MetadataEpoch {
        self.metadata_epoch
    }

    /// Returns contiguous persistence frontiers after this operation.
    #[must_use]
    pub const fn frontier(&self) -> WorldFrontierV1 {
        self.frontier
    }

    /// Returns stable-key-ordered per-chunk commit evidence.
    #[must_use]
    pub fn chunks(&self) -> &[ChunkCommitReceiptV1] {
        &self.chunks
    }

    /// Returns the durability boundary reached by this operation.
    #[must_use]
    pub const fn durability(&self) -> CommitDurabilityV1 {
        self.durability
    }

    /// Returns whether an exact retained transaction receipt was replayed.
    #[must_use]
    pub const fn replayed(&self) -> bool {
        self.replayed
    }
}

/// Commit result that never hides a DB-ahead/header-behind state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorldCommitOutcomeV1 {
    pub(crate) receipt: WorldCommitReceiptV1,
    pub(crate) header: CommitHeaderStatusV1,
}

impl WorldCommitOutcomeV1 {
    /// Returns authoritative DB commit details.
    #[must_use]
    pub const fn receipt(&self) -> &WorldCommitReceiptV1 {
        &self.receipt
    }

    /// Returns recoverable sidecar publication status.
    #[must_use]
    pub const fn header(&self) -> &CommitHeaderStatusV1 {
        &self.header
    }
}

/// Retention class of an independently verified checkpoint.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum CheckpointKindV1 {
    /// Initial, last-known-clean, pre-migration, or manually pinned checkpoint.
    Protected,
    /// Automatically rotated checkpoint; the fake enforces count bounds.
    RotatingAutomatic,
}

/// Request to create a named checkpoint at the current durable frontier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointRequestV1 {
    id: CheckpointId,
    kind: CheckpointKindV1,
    reason: String,
}

impl CheckpointRequestV1 {
    /// Creates a checkpoint request.
    #[must_use]
    pub fn new(id: CheckpointId, kind: CheckpointKindV1, reason: impl Into<String>) -> Self {
        Self {
            id,
            kind,
            reason: reason.into(),
        }
    }

    /// Returns the requested checkpoint identity.
    #[must_use]
    pub const fn id(&self) -> CheckpointId {
        self.id
    }

    /// Returns the checkpoint retention class.
    #[must_use]
    pub const fn kind(&self) -> CheckpointKindV1 {
        self.kind
    }

    /// Returns the stable human-readable creation reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

/// Verified independent checkpoint receipt.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointReceiptV1 {
    pub(crate) id: CheckpointId,
    pub(crate) kind: CheckpointKindV1,
    pub(crate) source_revision: WorldRevision,
    pub(crate) exact_lock_hash: DigestV1,
    pub(crate) metadata_hash: DigestV1,
    pub(crate) header_projection_hash: DigestV1,
    pub(crate) reason: String,
    pub(crate) physical_bytes: u64,
    pub(crate) content_hash: DigestV1,
    pub(crate) restore_verified: bool,
}

impl CheckpointReceiptV1 {
    /// Returns the checkpoint identity.
    #[must_use]
    pub const fn id(&self) -> CheckpointId {
        self.id
    }

    /// Returns the checkpoint retention class.
    #[must_use]
    pub const fn kind(&self) -> CheckpointKindV1 {
        self.kind
    }

    /// Returns the source world revision.
    #[must_use]
    pub const fn source_revision(&self) -> WorldRevision {
        self.source_revision
    }

    /// Returns the exact frozen-lock content hash captured by the checkpoint.
    #[must_use]
    pub const fn exact_lock_hash(&self) -> DigestV1 {
        self.exact_lock_hash
    }

    /// Returns the authoritative metadata hash captured by the checkpoint.
    #[must_use]
    pub const fn metadata_hash(&self) -> DigestV1 {
        self.metadata_hash
    }

    /// Returns the expected catalog-header projection hash.
    #[must_use]
    pub const fn header_projection_hash(&self) -> DigestV1 {
        self.header_projection_hash
    }

    /// Returns the stable human-readable checkpoint reason.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }

    /// Returns the physical bytes retained by the independent checkpoint.
    #[must_use]
    pub const fn physical_bytes(&self) -> u64 {
        self.physical_bytes
    }

    /// Returns the independent checkpoint image content hash.
    #[must_use]
    pub const fn content_hash(&self) -> DigestV1 {
        self.content_hash
    }

    /// Returns whether the independent image passed restore verification.
    #[must_use]
    pub const fn restore_verified(&self) -> bool {
        self.restore_verified
    }
}

/// Checkpoint result that preserves DB-first/header-behind recovery evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CheckpointOutcomeV1 {
    pub(crate) receipt: CheckpointReceiptV1,
    pub(crate) header: CommitHeaderStatusV1,
}

impl CheckpointOutcomeV1 {
    /// Returns the independently verified checkpoint receipt.
    #[must_use]
    pub const fn receipt(&self) -> &CheckpointReceiptV1 {
        &self.receipt
    }

    /// Returns the recoverable sidecar publication status.
    #[must_use]
    pub const fn header(&self) -> &CommitHeaderStatusV1 {
        &self.header
    }
}

pub(crate) fn next_revision(value: u64, counter: &'static str) -> WorldDbResult<u64> {
    value
        .checked_add(1)
        .ok_or(WorldDbError::RevisionOverflow { counter })
}

pub(crate) fn validate_metadata_limits(
    input: &AuthoritativeMetadataInputV1,
    limits: WorldStorageLimitsV1,
) -> WorldDbResult<()> {
    let lock_len = u64::try_from(input.frozen_lock.canonical_lock_bytes.len()).map_err(|_| {
        WorldDbError::LengthOverflow {
            what: "canonical frozen lock bytes",
        }
    })?;
    if lock_len > u64::from(limits.max_lock_bytes) {
        return Err(WorldDbError::MetadataLimitExceeded {
            what: "canonical frozen lock bytes",
            actual: lock_len,
            maximum: u64::from(limits.max_lock_bytes),
        });
    }
    let entry_count = input
        .requirement_closure
        .packages
        .len()
        .checked_add(input.requirement_closure.schemas.len())
        .and_then(|value| value.checked_add(input.requirement_closure.concrete_content.len()))
        .and_then(|value| {
            value.checked_add(input.requirement_closure.semantic_contract_majors.len())
        })
        .and_then(|value| value.checked_add(input.requirement_closure.generator_provenance.len()))
        .ok_or(WorldDbError::LengthOverflow {
            what: "requirement closure entry count",
        })?;
    let entry_count = u64::try_from(entry_count).map_err(|_| WorldDbError::LengthOverflow {
        what: "requirement closure entry count",
    })?;
    if entry_count > u64::from(limits.max_requirement_entries) {
        return Err(WorldDbError::MetadataLimitExceeded {
            what: "requirement closure entries",
            actual: entry_count,
            maximum: u64::from(limits.max_requirement_entries),
        });
    }
    Ok(())
}
