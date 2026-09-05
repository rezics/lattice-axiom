//! Versioned, bounded-decode durable store image for crash and checkpoint recovery.
//!
//! The image is the complete independent restore unit for the D3 reference
//! backend. It contains only portable identities, metadata, frontiers, and
//! `world-wire@1` chunk records. Writer leases are never encoded.

use std::collections::BTreeMap;

use latticeaxiom_core::WorldId;
use latticeaxiom_storage::WorldRevision;
use serde::{Deserialize, Serialize};

use crate::{
    AuthoritativeMetadataInputV1, CheckpointId, CheckpointReceiptV1, DigestV1, DisplayName,
    MetadataEpoch, StoreId, WorldDbError, WorldDbResult, WorldFrontierV1, WorldStorageLimitsV1,
};

/// Owner-controlled schema version of [`DurableStoreImageV1`].
pub const DURABLE_STORE_IMAGE_SCHEMA_VERSION_V1: u32 = 1;

const DURABLE_ROOT_DOMAIN: &[u8] = b"latticeaxiom/durable-root-image/v1";
const DURABLE_STORE_DOMAIN: &[u8] = b"latticeaxiom/durable-store-image/v1";

/// One world's complete durable records, metadata, and contiguous frontiers.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DurableStoreImageV1 {
    pub(crate) schema_version: u32,
    pub(crate) world: WorldId,
    pub(crate) display_name: DisplayName,
    pub(crate) store_id: StoreId,
    pub(crate) metadata_epoch: MetadataEpoch,
    pub(crate) metadata: AuthoritativeMetadataInputV1,
    pub(crate) metadata_hash: DigestV1,
    pub(crate) expected_header_hash: DigestV1,
    pub(crate) frontier: WorldFrontierV1,
    pub(crate) records: BTreeMap<Vec<u8>, Vec<u8>>,
}

impl DurableStoreImageV1 {
    pub(crate) fn from_parts(image: Self) -> WorldDbResult<Self> {
        let mut image = image;
        image.schema_version = DURABLE_STORE_IMAGE_SCHEMA_VERSION_V1;
        image.validate_bounds(WorldStorageLimitsV1::D3_BOOTSTRAP)?;
        Ok(image)
    }

    pub(crate) const fn world(&self) -> WorldId {
        self.world
    }

    pub(crate) const fn store_id(&self) -> &StoreId {
        &self.store_id
    }

    pub(crate) fn display_name(&self) -> DisplayName {
        self.display_name.clone()
    }

    pub(crate) const fn metadata_epoch(&self) -> MetadataEpoch {
        self.metadata_epoch
    }

    pub(crate) const fn metadata(&self) -> &AuthoritativeMetadataInputV1 {
        &self.metadata
    }

    pub(crate) const fn metadata_hash(&self) -> DigestV1 {
        self.metadata_hash
    }

    pub(crate) const fn expected_header_hash(&self) -> DigestV1 {
        self.expected_header_hash
    }

    pub(crate) const fn frontier(&self) -> WorldFrontierV1 {
        self.frontier
    }

    pub(crate) const fn records(&self) -> &BTreeMap<Vec<u8>, Vec<u8>> {
        &self.records
    }

    pub(crate) const fn source_revision(&self) -> WorldRevision {
        self.frontier.current()
    }

    pub(crate) fn content_hash(&self) -> WorldDbResult<DigestV1> {
        let bytes = encode_postcard(self, "durable store image v1")?;
        Ok(DigestV1::hash(DURABLE_STORE_DOMAIN, &bytes))
    }

    pub(crate) fn validate_bounds(&self, limits: WorldStorageLimitsV1) -> WorldDbResult<()> {
        if self.schema_version != DURABLE_STORE_IMAGE_SCHEMA_VERSION_V1 {
            return Err(WorldDbError::CorruptDurableImage {
                reason: format!(
                    "unsupported durable store image schema {}",
                    self.schema_version
                ),
            });
        }
        let record_count =
            u64::try_from(self.records.len()).map_err(|_| WorldDbError::LengthOverflow {
                what: "durable store record count",
            })?;
        if record_count > u64::from(limits.max_requirement_entries()) {
            return Err(WorldDbError::MetadataLimitExceeded {
                what: "durable store record count",
                actual: record_count,
                maximum: u64::from(limits.max_requirement_entries()),
            });
        }
        let mut total = 0_u64;
        for (key, value) in &self.records {
            let key_len = u64::try_from(key.len()).map_err(|_| WorldDbError::LengthOverflow {
                what: "durable store record key",
            })?;
            let value_len =
                u64::try_from(value.len()).map_err(|_| WorldDbError::LengthOverflow {
                    what: "durable store record value",
                })?;
            total = total
                .checked_add(key_len)
                .and_then(|sum| sum.checked_add(value_len))
                .ok_or(WorldDbError::LengthOverflow {
                    what: "durable store image bytes",
                })?;
            if total > limits.max_uncompressed_commit_bytes() {
                return Err(WorldDbError::TransactionPayloadLimitExceeded {
                    actual: total,
                    maximum: limits.max_uncompressed_commit_bytes(),
                });
            }
        }
        Ok(())
    }
}

/// Independently restorable world-store root, including retained checkpoints.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct DurableRootImageV1 {
    schema_version: u32,
    world: DurableStoreImageV1,
    checkpoints: BTreeMap<CheckpointId, DurableStoreImageV1>,
    checkpoint_receipts: BTreeMap<CheckpointId, CheckpointReceiptV1>,
}

impl DurableRootImageV1 {
    pub(crate) fn new(
        world: DurableStoreImageV1,
        checkpoints: BTreeMap<CheckpointId, DurableStoreImageV1>,
        checkpoint_receipts: BTreeMap<CheckpointId, CheckpointReceiptV1>,
    ) -> WorldDbResult<Self> {
        let root = Self {
            schema_version: DURABLE_STORE_IMAGE_SCHEMA_VERSION_V1,
            world,
            checkpoints,
            checkpoint_receipts,
        };
        root.validate()?;
        Ok(root)
    }

    pub(crate) const fn world(&self) -> &DurableStoreImageV1 {
        &self.world
    }

    pub(crate) const fn checkpoints(&self) -> &BTreeMap<CheckpointId, DurableStoreImageV1> {
        &self.checkpoints
    }

    pub(crate) fn checkpoint_receipts(&self) -> BTreeMap<CheckpointId, CheckpointReceiptV1> {
        self.checkpoint_receipts.clone()
    }

    pub(crate) fn encode(&self) -> WorldDbResult<(Vec<u8>, DigestV1)> {
        self.validate()?;
        let bytes = encode_postcard(self, "durable root image v1")?;
        let digest = DigestV1::hash(DURABLE_ROOT_DOMAIN, &bytes);
        Ok((bytes, digest))
    }

    pub(crate) fn decode(bytes: &[u8], expected: DigestV1) -> WorldDbResult<Self> {
        let actual = DigestV1::hash(DURABLE_ROOT_DOMAIN, bytes);
        if actual != expected {
            return Err(WorldDbError::CorruptDurableImage {
                reason: "durable root image digest mismatch".to_owned(),
            });
        }
        let root: Self =
            postcard::from_bytes(bytes).map_err(|error| WorldDbError::PostcardDecode {
                artifact: "durable root image v1",
                reason: error.to_string(),
            })?;
        root.validate()?;
        Ok(root)
    }

    fn validate(&self) -> WorldDbResult<()> {
        if self.schema_version != DURABLE_STORE_IMAGE_SCHEMA_VERSION_V1 {
            return Err(WorldDbError::CorruptDurableImage {
                reason: format!(
                    "unsupported durable root image schema {}",
                    self.schema_version
                ),
            });
        }
        self.world
            .validate_bounds(WorldStorageLimitsV1::D3_BOOTSTRAP)?;
        if self.checkpoints.len() != self.checkpoint_receipts.len() {
            return Err(WorldDbError::CorruptDurableImage {
                reason: "checkpoint image and receipt catalogs disagree".to_owned(),
            });
        }
        for (id, image) in &self.checkpoints {
            image.validate_bounds(WorldStorageLimitsV1::D3_BOOTSTRAP)?;
            let Some(receipt) = self.checkpoint_receipts.get(id) else {
                return Err(WorldDbError::CorruptDurableImage {
                    reason: "checkpoint image is missing its receipt".to_owned(),
                });
            };
            if image.world() != self.world.world()
                || image.store_id() != self.world.store_id()
                || receipt.source_revision() != image.source_revision()
            {
                return Err(WorldDbError::CorruptDurableImage {
                    reason: "checkpoint image identity disagrees with the live world".to_owned(),
                });
            }
        }
        Ok(())
    }
}

fn encode_postcard<T: Serialize>(value: &T, artifact: &'static str) -> WorldDbResult<Vec<u8>> {
    postcard::to_allocvec(value).map_err(|error| WorldDbError::postcard_encode(artifact, &error))
}
