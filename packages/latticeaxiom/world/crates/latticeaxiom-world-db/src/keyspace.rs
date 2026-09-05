//! Stable internal keyspace definitions for the version-one world database.
//!
//! Metadata keys are deliberately independent from Rust layouts and physical
//! database APIs. Their exact byte representation is:
//!
//! ```text
//! u16be metadata_key_major = 1
//! WorldId raw RFC 4122 bytes [16]
//! u16be metadata_key_kind
//! ```
//!
//! The persistent-entity index and idempotency receipt journal are each stored
//! as one explicitly versioned metadata value under their fixed world-and-kind
//! key. They are not split into ad-hoc record-key variants.
//!
//! Chunk-record keys are not defined again here. The only writable records
//! path validates the closed version-one record-kind set and then delegates to
//! `latticeaxiom-world-wire` so the portable contract has one encoder.

use latticeaxiom_core::WorldId;
use latticeaxiom_world_wire::{
    ChunkRecordKey, WireResult, WorldWireError, WorldWireLimits, encode_chunk_record_key,
};

/// Version written in the first two bytes of every metadata key.
pub const METADATA_KEY_MAJOR_V1: u16 = 1;

/// Exact byte length of a version-one metadata key.
pub const METADATA_KEY_LENGTH_V1: usize =
    METADATA_MAJOR_BYTES + WORLD_ID_BYTES + METADATA_KIND_BYTES;

/// Exact byte length of the prefix shared by one world's metadata keys.
pub const METADATA_WORLD_PREFIX_LENGTH_V1: usize = METADATA_MAJOR_BYTES + WORLD_ID_BYTES;
const METADATA_MAJOR_BYTES: usize = size_of::<u16>();
const WORLD_ID_BYTES: usize = 16;
const METADATA_KIND_BYTES: usize = size_of::<u16>();
const WORLD_ID_START: usize = METADATA_MAJOR_BYTES;
const WORLD_ID_END: usize = WORLD_ID_START + WORLD_ID_BYTES;
const METADATA_KIND_START: usize = WORLD_ID_END;

/// Column families used by the internal version-one database schema.
///
/// `RocksDB` always creates `default`; Lattice Axiom keeps it empty so all
/// authoritative entries are classified into an explicitly versioned family.
/// Unknown column families are not aliases for any member of this enum.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum ColumnFamilyV1 {
    /// `RocksDB`'s mandatory family, which must remain empty.
    Default,
    /// Authoritative world metadata keyed by [`MetadataKeyV1`].
    Metadata,
    /// Portable chunk records keyed by `latticeaxiom-world-wire`.
    Records,
}

impl ColumnFamilyV1 {
    /// Every family expected when opening a version-one database.
    pub const ALL: [Self; 3] = [Self::Default, Self::Metadata, Self::Records];

    /// Named families that must be created in addition to `RocksDB`'s default.
    pub const NAMED: [Self; 2] = [Self::Metadata, Self::Records];

    /// Returns the exact stable physical column-family name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Metadata => "latticeaxiom-metadata-v1",
            Self::Records => "latticeaxiom-records-v1",
        }
    }

    /// Returns whether this family must contain no keys on a valid open.
    ///
    /// A non-empty default family is rejected instead of being treated as an
    /// unversioned compatibility alias.
    #[must_use]
    pub const fn must_be_empty(self) -> bool {
        matches!(self, Self::Default)
    }
}

/// Stable version-one tags for authoritative metadata values.
///
/// The numeric values are part of the internal on-disk schema. New meanings
/// require unused tags; existing tags must never be renumbered or repurposed.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum MetadataKeyKindV1 {
    /// Complete authoritative metadata projection (`u16be` tag `1`).
    AuthoritativeMetadata,
    /// Monotonic metadata publication epoch (`u16be` tag `2`).
    MetadataEpoch,
    /// Expected canonical sidecar projection hash (`u16be` tag `3`).
    ExpectedHeaderProjectionHash,
    /// Clean-shutdown marker (`u16be` tag `4`).
    CleanMarker,
    /// Contiguous durable world-revision frontier (`u16be` tag `5`).
    DurableFrontier,
    /// Verified checkpoint catalog (`u16be` tag `6`).
    CheckpointCatalog,
    /// Complete versioned persistent-entity index (`u16be` tag `7`).
    PersistentEntityIndex,
    /// Complete versioned idempotency receipt journal (`u16be` tag `8`).
    TransactionReceiptJournal,
}

impl MetadataKeyKindV1 {
    /// Authoritative metadata projection tag.
    pub const AUTHORITATIVE_METADATA_TAG: u16 = 1;
    /// Metadata epoch tag.
    pub const METADATA_EPOCH_TAG: u16 = 2;
    /// Expected header projection hash tag.
    pub const EXPECTED_HEADER_PROJECTION_HASH_TAG: u16 = 3;
    /// Clean marker tag.
    pub const CLEAN_MARKER_TAG: u16 = 4;
    /// Durable frontier tag.
    pub const DURABLE_FRONTIER_TAG: u16 = 5;
    /// Checkpoint catalog tag.
    pub const CHECKPOINT_CATALOG_TAG: u16 = 6;
    /// Persistent-entity index tag.
    pub const PERSISTENT_ENTITY_INDEX_TAG: u16 = 7;
    /// Transaction receipt journal tag.
    pub const TRANSACTION_RECEIPT_JOURNAL_TAG: u16 = 8;

    /// All writable metadata kinds in stable tag order.
    pub const ALL: [Self; 8] = [
        Self::AuthoritativeMetadata,
        Self::MetadataEpoch,
        Self::ExpectedHeaderProjectionHash,
        Self::CleanMarker,
        Self::DurableFrontier,
        Self::CheckpointCatalog,
        Self::PersistentEntityIndex,
        Self::TransactionReceiptJournal,
    ];

    /// Returns the exact tag encoded in a metadata key.
    #[must_use]
    pub const fn tag(self) -> u16 {
        match self {
            Self::AuthoritativeMetadata => Self::AUTHORITATIVE_METADATA_TAG,
            Self::MetadataEpoch => Self::METADATA_EPOCH_TAG,
            Self::ExpectedHeaderProjectionHash => Self::EXPECTED_HEADER_PROJECTION_HASH_TAG,
            Self::CleanMarker => Self::CLEAN_MARKER_TAG,
            Self::DurableFrontier => Self::DURABLE_FRONTIER_TAG,
            Self::CheckpointCatalog => Self::CHECKPOINT_CATALOG_TAG,
            Self::PersistentEntityIndex => Self::PERSISTENT_ENTITY_INDEX_TAG,
            Self::TransactionReceiptJournal => Self::TRANSACTION_RECEIPT_JOURNAL_TAG,
        }
    }

    const fn from_tag(tag: u16) -> WireResult<Self> {
        match tag {
            Self::AUTHORITATIVE_METADATA_TAG => Ok(Self::AuthoritativeMetadata),
            Self::METADATA_EPOCH_TAG => Ok(Self::MetadataEpoch),
            Self::EXPECTED_HEADER_PROJECTION_HASH_TAG => Ok(Self::ExpectedHeaderProjectionHash),
            Self::CLEAN_MARKER_TAG => Ok(Self::CleanMarker),
            Self::DURABLE_FRONTIER_TAG => Ok(Self::DurableFrontier),
            Self::CHECKPOINT_CATALOG_TAG => Ok(Self::CheckpointCatalog),
            Self::PERSISTENT_ENTITY_INDEX_TAG => Ok(Self::PersistentEntityIndex),
            Self::TRANSACTION_RECEIPT_JOURNAL_TAG => Ok(Self::TransactionReceiptJournal),
            found => Err(WorldWireError::UnknownNumericTag {
                field: "metadata key kind",
                found,
            }),
        }
    }
}

/// Logical identity encoded by one version-one metadata key.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct MetadataKeyV1 {
    world: WorldId,
    kind: MetadataKeyKindV1,
}

impl MetadataKeyV1 {
    /// Creates a key from canonical world identity and a closed writable kind.
    #[must_use]
    pub const fn new(world: WorldId, kind: MetadataKeyKindV1) -> Self {
        Self { world, kind }
    }

    /// Returns the world whose metadata is addressed.
    #[must_use]
    pub const fn world(self) -> WorldId {
        self.world
    }

    /// Returns the stable metadata value kind.
    #[must_use]
    pub const fn kind(self) -> MetadataKeyKindV1 {
        self.kind
    }
}

/// Encodes one metadata key using the fixed version-one big-endian schema.
#[must_use]
pub fn encode_metadata_key(key: MetadataKeyV1) -> [u8; METADATA_KEY_LENGTH_V1] {
    let mut encoded = [0_u8; METADATA_KEY_LENGTH_V1];
    encoded[..METADATA_MAJOR_BYTES].copy_from_slice(&METADATA_KEY_MAJOR_V1.to_be_bytes());
    encoded[WORLD_ID_START..WORLD_ID_END].copy_from_slice(&key.world.as_bytes());
    encoded[METADATA_KIND_START..].copy_from_slice(&key.kind.tag().to_be_bytes());
    encoded
}

/// Decodes one exact version-one metadata key.
///
/// Unknown majors and kind tags are rejected instead of receiving writable
/// aliases. The fixed-length preflight happens before any field is read.
///
/// # Errors
///
/// Returns a typed world-wire error for truncation, trailing bytes, unknown
/// numeric tags, or invalid raw [`WorldId`] bytes.
pub fn decode_metadata_key(encoded: &[u8]) -> WireResult<MetadataKeyV1> {
    if encoded.len() < METADATA_KEY_LENGTH_V1 {
        return Err(WorldWireError::Truncated {
            section: "metadata key",
            needed: METADATA_KEY_LENGTH_V1,
            remaining: encoded.len(),
        });
    }
    if encoded.len() > METADATA_KEY_LENGTH_V1 {
        return Err(WorldWireError::TrailingKeyBytes {
            remaining: encoded.len() - METADATA_KEY_LENGTH_V1,
        });
    }

    let major = u16::from_be_bytes([encoded[0], encoded[1]]);
    if major != METADATA_KEY_MAJOR_V1 {
        return Err(WorldWireError::UnknownNumericTag {
            field: "metadata key major",
            found: major,
        });
    }

    let mut world_bytes = [0_u8; WORLD_ID_BYTES];
    world_bytes.copy_from_slice(&encoded[WORLD_ID_START..WORLD_ID_END]);
    let world =
        WorldId::from_bytes(world_bytes).map_err(|error| WorldWireError::InvalidWorldId {
            reason: error.to_string(),
        })?;
    let kind = MetadataKeyKindV1::from_tag(u16::from_be_bytes([
        encoded[METADATA_KIND_START],
        encoded[METADATA_KIND_START + 1],
    ]))?;
    Ok(MetadataKeyV1::new(world, kind))
}

/// Encodes the prefix shared by every version-one metadata key for `world`.
#[must_use]
pub fn metadata_world_prefix(world: WorldId) -> [u8; METADATA_WORLD_PREFIX_LENGTH_V1] {
    let mut prefix = [0_u8; METADATA_WORLD_PREFIX_LENGTH_V1];
    prefix[..METADATA_MAJOR_BYTES].copy_from_slice(&METADATA_KEY_MAJOR_V1.to_be_bytes());
    prefix[WORLD_ID_START..].copy_from_slice(&world.as_bytes());
    prefix
}

/// Encodes a writable records-family key through the portable wire contract.
///
/// # Errors
///
/// Returns [`WorldWireError::UnknownNumericTag`] for an opaque record kind, or
/// forwards the delegated encoder's length validation error.
pub(crate) fn encode_record_key_for_write(
    key: &ChunkRecordKey,
    limits: WorldWireLimits,
) -> WireResult<Vec<u8>> {
    if !key.record_kind().is_writable_v1() {
        return Err(WorldWireError::UnknownNumericTag {
            field: "writable chunk record kind",
            found: key.record_kind().raw(),
        });
    }
    encode_chunk_record_key(key, limits)
}

#[cfg(test)]
mod tests {
    use std::{error::Error, str::FromStr};

    use latticeaxiom_core::StableId;
    use latticeaxiom_storage::{ChunkCoordinate, ChunkKey, DimensionId};
    use latticeaxiom_world_wire::{RecordKind, decode_chunk_record_key, encode_chunk_record_key};

    use super::*;

    fn sample_world() -> Result<WorldId, Box<dyn Error>> {
        Ok(WorldId::from_str("018f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")?)
    }

    fn later_world() -> Result<WorldId, Box<dyn Error>> {
        Ok(WorldId::from_str("028f1e2d-3c4b-4a59-8c6d-7e8f9012abcd")?)
    }

    fn sample_record(kind: RecordKind) -> Result<ChunkRecordKey, Box<dyn Error>> {
        Ok(ChunkRecordKey::new(
            ChunkKey::new(
                sample_world()?,
                DimensionId::from_str("terrenia:dimension/terrenia")?,
                ChunkCoordinate::new(-2, 0, 3),
            ),
            kind,
            StableId::from_str("terrenia:schema/chunk-snapshot@1")?,
        ))
    }

    #[test]
    fn column_family_names_and_default_empty_policy_are_stable() {
        assert_eq!(ColumnFamilyV1::Default.name(), "default");
        assert_eq!(ColumnFamilyV1::Metadata.name(), "latticeaxiom-metadata-v1");
        assert_eq!(ColumnFamilyV1::Records.name(), "latticeaxiom-records-v1");
        assert!(ColumnFamilyV1::Default.must_be_empty());
        assert!(!ColumnFamilyV1::Metadata.must_be_empty());
        assert!(!ColumnFamilyV1::Records.must_be_empty());
        assert_eq!(
            ColumnFamilyV1::NAMED,
            [ColumnFamilyV1::Metadata, ColumnFamilyV1::Records]
        );
    }

    #[test]
    fn metadata_keys_round_trip_every_stable_kind() -> Result<(), Box<dyn Error>> {
        let world = sample_world()?;
        for kind in MetadataKeyKindV1::ALL {
            let key = MetadataKeyV1::new(world, kind);
            let encoded = encode_metadata_key(key);
            assert_eq!(decode_metadata_key(&encoded)?, key);
            assert_eq!(
                &encoded[..METADATA_WORLD_PREFIX_LENGTH_V1],
                metadata_world_prefix(world).as_slice()
            );
        }
        Ok(())
    }

    #[test]
    fn metadata_key_golden_uses_big_endian_major_world_and_kind() -> Result<(), Box<dyn Error>> {
        let encoded = encode_metadata_key(MetadataKeyV1::new(
            sample_world()?,
            MetadataKeyKindV1::ExpectedHeaderProjectionHash,
        ));
        assert_eq!(&encoded[..2], &[0x00, 0x01]);
        assert_eq!(
            &encoded[2..18],
            &[
                0x01, 0x8f, 0x1e, 0x2d, 0x3c, 0x4b, 0x4a, 0x59, 0x8c, 0x6d, 0x7e, 0x8f, 0x90, 0x12,
                0xab, 0xcd,
            ]
        );
        assert_eq!(&encoded[18..], &[0x00, 0x03]);
        let entity_index = encode_metadata_key(MetadataKeyV1::new(
            sample_world()?,
            MetadataKeyKindV1::PersistentEntityIndex,
        ));
        let receipt_journal = encode_metadata_key(MetadataKeyV1::new(
            sample_world()?,
            MetadataKeyKindV1::TransactionReceiptJournal,
        ));
        assert_eq!(&entity_index[18..], &[0x00, 0x07]);
        assert_eq!(&receipt_journal[18..], &[0x00, 0x08]);
        Ok(())
    }

    #[test]
    fn encoded_metadata_order_matches_world_then_stable_tag() -> Result<(), Box<dyn Error>> {
        let first_world = sample_world()?;
        let second_world = later_world()?;
        let first_kind = encode_metadata_key(MetadataKeyV1::new(
            first_world,
            MetadataKeyKindV1::AuthoritativeMetadata,
        ));
        let later_kind = encode_metadata_key(MetadataKeyV1::new(
            first_world,
            MetadataKeyKindV1::CheckpointCatalog,
        ));
        let later_world_first_kind = encode_metadata_key(MetadataKeyV1::new(
            second_world,
            MetadataKeyKindV1::AuthoritativeMetadata,
        ));

        assert!(first_kind < later_kind);
        assert!(later_kind < later_world_first_kind);
        Ok(())
    }

    #[test]
    fn world_prefix_selects_only_that_worlds_metadata() -> Result<(), Box<dyn Error>> {
        let world = sample_world()?;
        let prefix = metadata_world_prefix(world);
        for kind in MetadataKeyKindV1::ALL {
            assert!(encode_metadata_key(MetadataKeyV1::new(world, kind)).starts_with(&prefix));
        }
        assert!(
            !encode_metadata_key(MetadataKeyV1::new(
                later_world()?,
                MetadataKeyKindV1::AuthoritativeMetadata,
            ))
            .starts_with(&prefix)
        );
        Ok(())
    }

    #[test]
    fn decoder_rejects_truncation_trailing_unknown_and_invalid_identity()
    -> Result<(), Box<dyn Error>> {
        let encoded = encode_metadata_key(MetadataKeyV1::new(
            sample_world()?,
            MetadataKeyKindV1::AuthoritativeMetadata,
        ));
        for length in 0..METADATA_KEY_LENGTH_V1 {
            assert!(decode_metadata_key(&encoded[..length]).is_err());
        }

        let mut trailing = encoded.to_vec();
        trailing.push(0);
        assert!(matches!(
            decode_metadata_key(&trailing),
            Err(WorldWireError::TrailingKeyBytes { remaining: 1 })
        ));

        let mut unknown_major = encoded;
        unknown_major[..2].copy_from_slice(&2_u16.to_be_bytes());
        assert!(matches!(
            decode_metadata_key(&unknown_major),
            Err(WorldWireError::UnknownNumericTag {
                field: "metadata key major",
                found: 2,
            })
        ));

        let mut unknown_kind = encoded;
        unknown_kind[18..].copy_from_slice(&u16::MAX.to_be_bytes());
        assert!(matches!(
            decode_metadata_key(&unknown_kind),
            Err(WorldWireError::UnknownNumericTag {
                field: "metadata key kind",
                found: u16::MAX,
            })
        ));

        let mut invalid_world = encoded;
        invalid_world[WORLD_ID_START..WORLD_ID_END].fill(0);
        assert!(matches!(
            decode_metadata_key(&invalid_world),
            Err(WorldWireError::InvalidWorldId { .. })
        ));
        Ok(())
    }

    #[test]
    fn writable_record_keys_delegate_exactly_and_opaque_kinds_fail_closed()
    -> Result<(), Box<dyn Error>> {
        let writable = sample_record(RecordKind::CHUNK_SNAPSHOT)?;
        let limits = WorldWireLimits::default();
        let delegated = encode_chunk_record_key(&writable, limits)?;
        let encoded = encode_record_key_for_write(&writable, limits)?;
        assert_eq!(encoded, delegated);
        assert_eq!(decode_chunk_record_key(&encoded, limits)?, writable);

        let opaque = sample_record(RecordKind::from_raw(77))?;
        assert!(matches!(
            encode_record_key_for_write(&opaque, limits),
            Err(WorldWireError::UnknownNumericTag {
                field: "writable chunk record kind",
                found: 77,
            })
        ));
        Ok(())
    }
}
