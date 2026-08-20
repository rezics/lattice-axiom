use std::fmt;

use latticeaxiom_core::{
    CanonicalHash, CanonicalJsonError, WorldId, canonical_json_bytes, canonical_json_hash,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

use crate::DisplayName;

/// Fixed sidecar file name owned by the world catalog contract.
pub const WORLD_HEADER_FILE_NAME: &str = "world-header.json";
/// Current bounded world-header schema version.
pub const WORLD_HEADER_SCHEMA_VERSION: u32 = 1;
/// Maximum encoded bytes accepted from a world-header sidecar.
pub const MAX_WORLD_HEADER_BYTES: usize = 64 * 1024;
const MAX_OPAQUE_ID_BYTES: usize = 128;

/// Bounded opaque storage-generation identity.
///
/// ADR 0027 does not assign a textual grammar to `store_id`; this wrapper only
/// enforces the bounded, path-independent properties needed by the catalog.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct StoreId(String);

impl StoreId {
    /// Validates a stable opaque store identifier.
    ///
    /// # Errors
    ///
    /// Returns [`OpaqueIdError`] for an empty, oversized, non-ASCII, control,
    /// whitespace, or path-like value.
    pub fn new(value: &str) -> Result<Self, OpaqueIdError> {
        validate_opaque_id(value, "store ID")?;
        Ok(Self(value.to_owned()))
    }

    /// Returns the uninterpreted identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Stable identifier of one checkpoint within a world.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CheckpointId(String);

impl CheckpointId {
    /// Validates a stable opaque checkpoint identifier.
    ///
    /// # Errors
    ///
    /// Returns [`OpaqueIdError`] when the value is not a bounded,
    /// path-independent token.
    pub fn new(value: &str) -> Result<Self, OpaqueIdError> {
        validate_opaque_id(value, "checkpoint ID")?;
        Ok(Self(value.to_owned()))
    }

    /// Returns the uninterpreted identifier.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

macro_rules! impl_string_id {
    ($type:ty, $label:literal) => {
        impl fmt::Display for $type {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(self.as_str())
            }
        }

        impl Serialize for $type {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $type {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let value = String::deserialize(deserializer)?;
                Self::new(&value).map_err(de::Error::custom)
            }
        }
    };
}

impl_string_id!(StoreId, "store ID");
impl_string_id!(CheckpointId, "checkpoint ID");

/// Failure to validate a bounded opaque identifier.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum OpaqueIdError {
    /// The identifier is empty.
    #[error("{kind} must not be empty")]
    Empty {
        /// Human-readable identifier kind.
        kind: &'static str,
    },
    /// The identifier exceeds the fixed bound.
    #[error("{kind} has {actual} bytes; the limit is 128")]
    TooLong {
        /// Human-readable identifier kind.
        kind: &'static str,
        /// Observed byte length.
        actual: usize,
    },
    /// The identifier is not a safe opaque ASCII token.
    #[error("{kind} contains a forbidden character")]
    ForbiddenCharacter {
        /// Human-readable identifier kind.
        kind: &'static str,
    },
}

fn validate_opaque_id(value: &str, kind: &'static str) -> Result<(), OpaqueIdError> {
    if value.is_empty() {
        return Err(OpaqueIdError::Empty { kind });
    }
    if value.len() > MAX_OPAQUE_ID_BYTES {
        return Err(OpaqueIdError::TooLong {
            kind,
            actual: value.len(),
        });
    }
    if !value.is_ascii()
        || value.bytes().any(|byte| {
            byte.is_ascii_control() || byte.is_ascii_whitespace() || matches!(byte, b'/' | b'\\')
        })
    {
        return Err(OpaqueIdError::ForbiddenCharacter { kind });
    }
    Ok(())
}

/// Exact fingerprints projected into the bounded world header.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldFingerprints {
    /// Frozen exact lock fingerprint.
    pub exact_lock: CanonicalHash,
    /// Registration image fingerprint.
    pub registration: CanonicalHash,
    /// Semantic image fingerprint.
    pub semantic: CanonicalHash,
    /// Authoritative settings fingerprint.
    pub settings: CanonicalHash,
}

/// Bounded projection of catalog-visible checkpoints.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointSummary {
    /// Most recent checkpoint, when one exists.
    pub latest: Option<CheckpointId>,
    /// Number of checkpoint entries represented by authoritative metadata.
    pub count: u32,
    /// Source revision of `latest`.
    pub latest_revision: Option<u64>,
    /// Physical bytes retained by the represented checkpoints.
    pub physical_bytes: u64,
}

/// All sidecar fields covered by the projection checksum.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeaderProjectionV1 {
    /// Header schema version; must be [`WORLD_HEADER_SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Immutable `UUIDv4` world identity.
    pub world_id: WorldId,
    /// Storage-generation identity.
    pub store_id: StoreId,
    /// User-facing name, independent of directory identity.
    pub display_name: DisplayName,
    /// Monotonic authoritative metadata epoch.
    pub metadata_epoch: u64,
    /// Hash of the complete authoritative metadata body.
    pub authoritative_metadata_hash: CanonicalHash,
    /// Exact lock, registration, semantic, and settings projections.
    pub fingerprints: WorldFingerprints,
    /// Whether the authoritative store reached a clean shutdown marker.
    pub clean_shutdown: bool,
    /// Latest authoritative revision present in metadata.
    pub authoritative_revision: u64,
    /// Contiguous durable frontier.
    pub durable_frontier: u64,
    /// Bounded checkpoint catalog projection.
    pub checkpoints: CheckpointSummary,
}

/// Canonical, bounded `world-header.json` document.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorldHeaderV1 {
    /// Fields projected from authoritative metadata.
    pub projection: HeaderProjectionV1,
    /// SHA-256 of canonical JSON for `projection`.
    pub checksum: CanonicalHash,
}

impl WorldHeaderV1 {
    /// Seals a projection with its canonical checksum.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if canonical serialization fails.
    pub fn seal(projection: HeaderProjectionV1) -> Result<Self, CanonicalJsonError> {
        let checksum = canonical_json_hash(&projection)?;
        Ok(Self {
            projection,
            checksum,
        })
    }

    /// Recomputes the projection checksum.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if canonical serialization fails.
    pub fn recompute_checksum(&self) -> Result<CanonicalHash, CanonicalJsonError> {
        canonical_json_hash(&self.projection)
    }

    /// Encodes the complete header as compact canonical JSON.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderCodecError`] if the stored checksum is wrong, encoding
    /// fails, or the resulting sidecar exceeds [`MAX_WORLD_HEADER_BYTES`].
    pub fn encode_canonical(&self) -> Result<Vec<u8>, HeaderCodecError> {
        self.verify_checksum()?;
        let bytes = canonical_json_bytes(self).map_err(HeaderCodecError::canonical)?;
        if bytes.len() > MAX_WORLD_HEADER_BYTES {
            return Err(HeaderCodecError::TooLarge {
                actual: bytes.len(),
            });
        }
        Ok(bytes)
    }

    /// Decodes, bounds, and validates an exact canonical sidecar.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderCodecError`] for oversized, malformed, noncanonical,
    /// unsupported, or checksum-invalid input.
    pub fn decode_canonical(bytes: &[u8]) -> Result<Self, HeaderCodecError> {
        if bytes.len() > MAX_WORLD_HEADER_BYTES {
            return Err(HeaderCodecError::TooLarge {
                actual: bytes.len(),
            });
        }
        let header = serde_json::from_slice::<Self>(bytes)
            .map_err(|error| HeaderCodecError::Malformed(error.to_string()))?;
        if header.projection.schema_version != WORLD_HEADER_SCHEMA_VERSION {
            return Err(HeaderCodecError::UnsupportedSchema {
                observed: header.projection.schema_version,
            });
        }
        header.verify_checksum()?;
        let canonical = canonical_json_bytes(&header).map_err(HeaderCodecError::canonical)?;
        if canonical != bytes {
            return Err(HeaderCodecError::NonCanonical);
        }
        Ok(header)
    }

    fn verify_checksum(&self) -> Result<(), HeaderCodecError> {
        let actual = self
            .recompute_checksum()
            .map_err(HeaderCodecError::canonical)?;
        if actual != self.checksum {
            return Err(HeaderCodecError::BadChecksum {
                claimed: self.checksum,
                actual,
            });
        }
        Ok(())
    }
}

/// Read-only authoritative metadata needed to cross-check a sidecar.
///
/// A storage adapter obtains this value without opening a writer. The expected
/// projection hash is committed in the authoritative DB-first batch.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AuthoritativeMetadataV1 {
    /// Exact header projection derived from authoritative metadata.
    pub projected_header: HeaderProjectionV1,
    /// Projection hash committed with the metadata epoch.
    pub expected_header_projection_hash: CanonicalHash,
}

impl AuthoritativeMetadataV1 {
    /// Builds internally consistent read-only metadata evidence.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalJsonError`] if canonical hashing fails.
    pub fn seal(projected_header: HeaderProjectionV1) -> Result<Self, CanonicalJsonError> {
        let expected_header_projection_hash = canonical_json_hash(&projected_header)?;
        Ok(Self {
            projected_header,
            expected_header_projection_hash,
        })
    }

    /// Validates the DB-committed expected projection hash.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderCodecError`] when hashing fails or metadata carries an
    /// inconsistent expected hash.
    pub fn verify_projection_hash(&self) -> Result<(), HeaderCodecError> {
        let actual =
            canonical_json_hash(&self.projected_header).map_err(HeaderCodecError::canonical)?;
        if actual != self.expected_header_projection_hash {
            return Err(HeaderCodecError::BadMetadataProjectionHash {
                claimed: self.expected_header_projection_hash,
                actual,
            });
        }
        Ok(())
    }

    /// Rebuilds a sealed sidecar from authoritative read-only metadata.
    ///
    /// # Errors
    ///
    /// Returns [`HeaderCodecError`] if the metadata projection hash is invalid.
    pub fn rebuild_header(&self) -> Result<WorldHeaderV1, HeaderCodecError> {
        self.verify_projection_hash()?;
        Ok(WorldHeaderV1 {
            projection: self.projected_header.clone(),
            checksum: self.expected_header_projection_hash,
        })
    }
}

/// Failure to decode or validate a bounded world header.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum HeaderCodecError {
    /// Encoded bytes exceed the sidecar limit.
    #[error("world header has {actual} bytes; the limit is 65536")]
    TooLarge {
        /// Observed byte length.
        actual: usize,
    },
    /// JSON or typed DTO decoding failed.
    #[error("malformed world header: {0}")]
    Malformed(String),
    /// The schema version is not understood by this reader.
    #[error("unsupported world-header schema {observed}")]
    UnsupportedSchema {
        /// Version read from the sidecar.
        observed: u32,
    },
    /// Bytes are valid JSON but not the canonical encoding.
    #[error("world header is not encoded as canonical JSON")]
    NonCanonical,
    /// The sidecar checksum does not match its projection.
    #[error("world header checksum mismatch: claimed {claimed}, actual {actual}")]
    BadChecksum {
        /// Checksum read from the sidecar.
        claimed: CanonicalHash,
        /// Recomputed checksum.
        actual: CanonicalHash,
    },
    /// Authoritative metadata contains an inconsistent expected projection.
    #[error("metadata projection hash mismatch: claimed {claimed}, actual {actual}")]
    BadMetadataProjectionHash {
        /// Hash committed in metadata.
        claimed: CanonicalHash,
        /// Recomputed hash of the metadata projection.
        actual: CanonicalHash,
    },
    /// Canonical serialization failed.
    #[error("canonical header encoding failed: {0}")]
    Canonical(String),
}

impl HeaderCodecError {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "Result::map_err supplies the owned serialization error"
    )]
    fn canonical(error: CanonicalJsonError) -> Self {
        Self::Canonical(error.to_string())
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn canonical_codec_rejects_mutation_and_noncanonical_json() {
        let projection = fixture_projection();
        let header = WorldHeaderV1::seal(projection).unwrap_or_else(|error| panic!("{error}"));
        let bytes = header
            .encode_canonical()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            WorldHeaderV1::decode_canonical(&bytes).ok(),
            Some(header.clone())
        );

        let mut changed = header;
        changed.projection.durable_frontier += 1;
        assert!(matches!(
            changed.encode_canonical(),
            Err(HeaderCodecError::BadChecksum { .. })
        ));

        let mut padded = bytes;
        padded.push(b'\n');
        assert_eq!(
            WorldHeaderV1::decode_canonical(&padded),
            Err(HeaderCodecError::NonCanonical)
        );
    }

    pub(crate) fn fixture_projection() -> HeaderProjectionV1 {
        let world_id = "123e4567-e89b-42d3-a456-426614174000"
            .parse()
            .unwrap_or_else(|error| panic!("fixture world ID: {error}"));
        HeaderProjectionV1 {
            schema_version: WORLD_HEADER_SCHEMA_VERSION,
            world_id,
            store_id: StoreId::new("store-1").unwrap_or_else(|error| panic!("{error}")),
            display_name: DisplayName::new("D3 World").unwrap_or_else(|error| panic!("{error}")),
            metadata_epoch: 7,
            authoritative_metadata_hash: CanonicalHash::digest(b"metadata"),
            fingerprints: WorldFingerprints {
                exact_lock: CanonicalHash::digest(b"lock"),
                registration: CanonicalHash::digest(b"registration"),
                semantic: CanonicalHash::digest(b"semantic"),
                settings: CanonicalHash::digest(b"settings"),
            },
            clean_shutdown: true,
            authoritative_revision: 11,
            durable_frontier: 11,
            checkpoints: CheckpointSummary {
                latest: Some(
                    CheckpointId::new("checkpoint-1").unwrap_or_else(|error| panic!("{error}")),
                ),
                count: 1,
                latest_revision: Some(11),
                physical_bytes: 4096,
            },
        }
    }
}
