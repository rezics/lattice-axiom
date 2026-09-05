use std::{collections::BTreeMap, fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

/// A SHA-256 digest used for canonical Lattice Axiom data.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalHash([u8; 32]);

impl CanonicalHash {
    /// The byte length of a SHA-256 digest.
    pub const BYTE_LENGTH: usize = 32;

    /// Computes a SHA-256 digest over the exact supplied bytes.
    #[must_use]
    pub fn digest(bytes: impl AsRef<[u8]>) -> Self {
        let digest = Sha256::digest(bytes.as_ref());
        let mut output = [0_u8; Self::BYTE_LENGTH];
        output.copy_from_slice(&digest);
        Self(output)
    }

    /// Creates a canonical hash from an already validated SHA-256 byte array.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; Self::BYTE_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Returns the digest bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; Self::BYTE_LENGTH] {
        &self.0
    }

    /// Consumes the hash and returns its digest bytes.
    #[must_use]
    pub const fn into_bytes(self) -> [u8; Self::BYTE_LENGTH] {
        self.0
    }
}

impl fmt::Display for CanonicalHash {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl FromStr for CanonicalHash {
    type Err = CanonicalHashParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != Self::BYTE_LENGTH * 2 {
            return Err(CanonicalHashParseError::InvalidLength {
                actual: value.len(),
            });
        }

        let mut bytes = [0_u8; Self::BYTE_LENGTH];
        hex::decode_to_slice(value, &mut bytes).map_err(|error| {
            CanonicalHashParseError::InvalidHex {
                reason: error.to_string(),
            }
        })?;
        Ok(Self(bytes))
    }
}

impl Serialize for CanonicalHash {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for CanonicalHash {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// An error produced while parsing a hexadecimal [`CanonicalHash`].
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CanonicalHashParseError {
    /// The input did not contain exactly 64 hexadecimal characters.
    #[error("a SHA-256 hash requires 64 hexadecimal characters, got {actual}")]
    InvalidLength {
        /// The number of bytes in the rejected text.
        actual: usize,
    },
    /// The input had the correct length but contained invalid hexadecimal text.
    #[error("invalid hexadecimal SHA-256 hash: {reason}")]
    InvalidHex {
        /// The hexadecimal decoder diagnostic.
        reason: String,
    },
}

/// An error produced while converting a value to canonical JSON.
#[derive(Debug, Error)]
#[error("failed to encode canonical JSON: {source}")]
pub struct CanonicalJsonError {
    #[source]
    source: serde_json::Error,
}

impl From<serde_json::Error> for CanonicalJsonError {
    fn from(source: serde_json::Error) -> Self {
        Self { source }
    }
}

/// Encodes a serializable value as deterministic compact JSON.
///
/// Object keys are sorted recursively before encoding. Arrays preserve their
/// declared order, so callers must sort any semantically unordered sequence
/// before invoking this function. Callers also choose the semantic payload:
/// source paths and provenance are excluded only when the caller hashes a DTO
/// that does not contain those fields.
///
/// # Errors
///
/// Returns an error when `value` cannot be represented as JSON.
pub fn canonical_json_bytes<T>(value: &T) -> Result<Vec<u8>, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    let json = serde_json::to_value(value).map_err(|source| CanonicalJsonError { source })?;
    serde_json::to_vec(&sort_json_objects(json)).map_err(|source| CanonicalJsonError { source })
}

/// Computes a [`CanonicalHash`] over [`canonical_json_bytes`].
///
/// # Errors
///
/// Returns an error when `value` cannot be represented as JSON.
pub fn canonical_json_hash<T>(value: &T) -> Result<CanonicalHash, CanonicalJsonError>
where
    T: Serialize + ?Sized,
{
    canonical_json_bytes(value).map(CanonicalHash::digest)
}

fn sort_json_objects(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(sort_json_objects).collect()),
        Value::Object(entries) => {
            let sorted = entries
                .into_iter()
                .map(|(key, value)| (key, sort_json_objects(value)))
                .collect::<BTreeMap<_, _>>();
            Value::Object(sorted.into_iter().collect())
        }
        scalar => scalar,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde::Serialize;

    use super::*;
    use crate::{SourceProvenance, SourceSpan};

    #[test]
    fn digest_matches_the_sha_256_reference_vector() {
        assert_eq!(
            CanonicalHash::digest(b"abc").to_string(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn canonical_hash_round_trips_and_revalidates_json() {
        let hash = CanonicalHash::digest(b"round-trip");
        let encoded = serde_json::to_string(&hash).unwrap_or_default();
        assert_eq!(
            serde_json::from_str::<CanonicalHash>(&encoded).ok(),
            Some(hash)
        );
        assert!(serde_json::from_str::<CanonicalHash>(r#""short""#).is_err());
        assert!(
            serde_json::from_str::<CanonicalHash>(
                r#""zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz""#
            )
            .is_err()
        );
    }

    #[test]
    fn btree_map_insertion_order_does_not_change_canonical_bytes_or_hash() {
        let mut forward = BTreeMap::new();
        forward.insert("air", 0_u8);
        forward.insert("stone", 1_u8);

        let mut reverse = BTreeMap::new();
        reverse.insert("stone", 1_u8);
        reverse.insert("air", 0_u8);

        assert_eq!(
            canonical_json_bytes(&forward).ok(),
            canonical_json_bytes(&reverse).ok()
        );
        assert_eq!(
            canonical_json_hash(&forward).ok(),
            canonical_json_hash(&reverse).ok()
        );
    }

    #[test]
    fn caller_can_exclude_source_provenance_from_semantic_payload_hash() {
        #[derive(Serialize)]
        struct SemanticPayload {
            entries: BTreeMap<&'static str, u8>,
        }

        #[derive(Serialize)]
        struct AuthoredValue<'a> {
            payload: &'a SemanticPayload,
            provenance: SourceProvenance,
        }

        let payload = SemanticPayload {
            entries: BTreeMap::from([("stone", 1)]),
        };
        let first = AuthoredValue {
            payload: &payload,
            provenance: SourceProvenance::new(
                "latticeaxiom:source/a"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
                "packages/a/package.ncl",
                CanonicalHash::digest(b"first source"),
                Some(
                    SourceSpan::new(10, 20)
                        .unwrap_or_else(|error| panic!("valid source span was rejected: {error}")),
                ),
                Vec::new(),
            )
            .unwrap_or_else(|error| panic!("valid provenance was rejected: {error}")),
        };
        let second = AuthoredValue {
            payload: &payload,
            provenance: SourceProvenance::new(
                "latticeaxiom:source/b"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
                "moved/package.ncl",
                CanonicalHash::digest(b"second source"),
                Some(
                    SourceSpan::new(100, 120)
                        .unwrap_or_else(|error| panic!("valid source span was rejected: {error}")),
                ),
                Vec::new(),
            )
            .unwrap_or_else(|error| panic!("valid provenance was rejected: {error}")),
        };

        let semantic_hash = canonical_json_hash(&payload).ok();
        assert_eq!(semantic_hash, canonical_json_hash(first.payload).ok());
        assert_eq!(semantic_hash, canonical_json_hash(second.payload).ok());
        assert_ne!(
            canonical_json_hash(&first).ok(),
            canonical_json_hash(&second).ok()
        );
    }
}
