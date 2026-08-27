use std::fmt;

use latticeaxiom_core::CanonicalHash;
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use unicode_normalization::UnicodeNormalization;

use crate::hashes::concatenated_hash;

const INTEGER_DOMAIN: &[u8] = b"latticeaxiom.world-seed.integer.v1\0";
const TEXT_DOMAIN: &[u8] = b"latticeaxiom.world-seed.text.v1\0";
const WORLDGEN_SEED_ROOT_DOMAIN: &[u8] = b"latticeaxiom.worldgen.seed-root.v2\0";

/// Exact 32-byte world seed consumed by version-one generation algorithms.
///
/// Randomness is deliberately injected by the caller through
/// [`Self::from_csprng_bytes`]. This pure crate neither owns an operating-system
/// random source nor delays persistence of the resulting bytes.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorldSeedV1([u8; CanonicalHash::BYTE_LENGTH]);

impl WorldSeedV1 {
    /// Creates a seed from bytes supplied by an operating-system CSPRNG.
    #[must_use]
    pub const fn from_csprng_bytes(bytes: [u8; CanonicalHash::BYTE_LENGTH]) -> Self {
        Self(bytes)
    }

    /// Derives a seed from the canonical signed decimal form of an integer.
    #[must_use]
    pub fn from_integer(value: i64) -> Self {
        let canonical = value.to_string();
        Self(concatenated_hash(INTEGER_DOMAIN, &[canonical.as_bytes()]).into_bytes())
    }

    /// Derives a seed from valid UTF-8 text normalized to Unicode NFC.
    #[must_use]
    pub fn from_text(value: &str) -> Self {
        let normalized = value.nfc().collect::<String>();
        Self(concatenated_hash(TEXT_DOMAIN, &[normalized.as_bytes()]).into_bytes())
    }

    /// Returns the exact persisted seed bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CanonicalHash::BYTE_LENGTH] {
        &self.0
    }
}

impl fmt::Display for WorldSeedV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        CanonicalHash::from_bytes(self.0).fmt(formatter)
    }
}

/// Version-two root for deterministic world-generation field seeds.
///
/// Only the persisted world seed contributes to this root. Generator
/// revisions, provider fingerprints, package locks, and activation identities
/// belong to provenance and compatibility checks; they must not silently
/// re-roll unrelated terrain fields.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorldgenSeedRootV2([u8; CanonicalHash::BYTE_LENGTH]);

impl WorldgenSeedRootV2 {
    /// Derives the version-two field root from a persisted world seed.
    #[must_use]
    pub fn from_world_seed(seed: WorldSeedV1) -> Self {
        Self(concatenated_hash(WORLDGEN_SEED_ROOT_DOMAIN, &[seed.as_bytes()]).into_bytes())
    }

    /// Returns the exact root bytes consumed by stable field domains.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; CanonicalHash::BYTE_LENGTH] {
        &self.0
    }
}

impl Serialize for WorldSeedV1 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for WorldSeedV1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let hash = value.parse::<CanonicalHash>().map_err(de::Error::custom)?;
        Ok(Self(hash.into_bytes()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_derivation_matches_independent_known_answer_vectors() {
        assert_eq!(
            WorldSeedV1::from_integer(42).to_string(),
            "a77950ff8fe50a0be0732447c7ce616f5b860bd30cd595241b62f3f24a7a24de"
        );
        assert_eq!(
            WorldSeedV1::from_integer(-42).to_string(),
            "86159ad0d4d993af6a95c5cdb8dbac45518094180b5818f83f3d34453e24b6af"
        );
        assert_eq!(
            WorldSeedV1::from_text("42").to_string(),
            "57770a87fed3bc51b922c40b8841f332ef898930d681e6181f5fb046cc0aecf3"
        );
        assert_eq!(
            WorldSeedV1::from_text("é").to_string(),
            "0071c5dafc3cab2c5a330e8864312f22f49ffdb54c666ea1b5efabc11eeed75a"
        );
    }

    #[test]
    fn integer_and_text_namespaces_are_separate() {
        assert_ne!(WorldSeedV1::from_integer(42), WorldSeedV1::from_text("42"));
    }

    #[test]
    fn text_is_normalized_to_nfc() {
        assert_eq!(
            WorldSeedV1::from_text("e\u{301}"),
            WorldSeedV1::from_text("é")
        );
    }

    #[test]
    fn serialized_seed_round_trips_as_exact_hex() {
        let seed = WorldSeedV1::from_integer(-42);
        let encoded = serde_json::to_string(&seed).unwrap_or_default();
        assert_eq!(serde_json::from_str(&encoded).ok(), Some(seed));
        assert_eq!(encoded.len(), 66);
    }

    #[test]
    fn worldgen_v2_root_depends_only_on_the_world_seed() {
        let first = WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(42));
        let second = WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(42));
        let other = WorldgenSeedRootV2::from_world_seed(WorldSeedV1::from_integer(43));

        assert_eq!(first, second);
        assert_ne!(first, other);
    }
}
