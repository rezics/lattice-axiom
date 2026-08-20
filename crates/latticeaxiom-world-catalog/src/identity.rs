use std::{fmt, str::FromStr};

use latticeaxiom_core::{IdentifierError, WorldId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// Maximum number of Unicode scalar values in a world display name.
pub const MAX_DISPLAY_NAME_SCALARS: usize = 128;
/// Maximum UTF-8 byte length of a world display name.
pub const MAX_DISPLAY_NAME_BYTES: usize = 512;

/// Canonical directory segment for one live world.
///
/// The segment is always exactly the lowercase, hyphenated `UUIDv4` text of the
/// world identity. A display name never participates in this value.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorldDirectoryName(WorldId);

impl WorldDirectoryName {
    /// Builds the canonical directory segment for `world_id`.
    #[must_use]
    pub const fn for_world(world_id: WorldId) -> Self {
        Self(world_id)
    }

    /// Returns the immutable identity encoded by this directory segment.
    #[must_use]
    pub const fn world_id(self) -> WorldId {
        self.0
    }
}

impl fmt::Display for WorldDirectoryName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl FromStr for WorldDirectoryName {
    type Err = IdentifierError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        value.parse().map(Self)
    }
}

impl Serialize for WorldDirectoryName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for WorldDirectoryName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// Validated, canonical user-facing world name.
///
/// Construction trims surrounding Unicode whitespace and normalizes the
/// result to NFC. Persistent deserialization is stricter and requires input
/// that is already in this canonical form.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DisplayName(String);

impl DisplayName {
    /// Normalizes and validates a user-provided display name.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayNameError`] for empty names, control characters, path
    /// separators, or a value exceeding either bounded length.
    pub fn new(value: &str) -> Result<Self, DisplayNameError> {
        if let Some(character) = value.chars().find(|character| character.is_control()) {
            return Err(DisplayNameError::ControlCharacter {
                codepoint: u32::from(character),
            });
        }

        let normalized = value.trim().nfc().collect::<String>();
        Self::validate_canonical(&normalized)?;
        Ok(Self(normalized))
    }

    /// Parses a persistent display name that must already be trimmed and NFC.
    ///
    /// # Errors
    ///
    /// Returns [`DisplayNameError`] if `value` is invalid or would change
    /// during canonicalization.
    pub fn parse_canonical(value: &str) -> Result<Self, DisplayNameError> {
        let parsed = Self::new(value)?;
        if parsed.as_str() != value {
            return Err(DisplayNameError::NonCanonical);
        }
        Ok(parsed)
    }

    /// Returns the canonical user-facing value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn validate_canonical(value: &str) -> Result<(), DisplayNameError> {
        if value.is_empty() {
            return Err(DisplayNameError::Empty);
        }
        if value.contains(['/', '\\']) {
            return Err(DisplayNameError::PathSeparator);
        }
        let scalar_count = value.chars().count();
        if scalar_count > MAX_DISPLAY_NAME_SCALARS {
            return Err(DisplayNameError::TooManyScalars {
                actual: scalar_count,
            });
        }
        if value.len() > MAX_DISPLAY_NAME_BYTES {
            return Err(DisplayNameError::TooManyBytes {
                actual: value.len(),
            });
        }
        Ok(())
    }
}

impl fmt::Display for DisplayName {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Serialize for DisplayName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DisplayName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse_canonical(&value).map_err(de::Error::custom)
    }
}

/// Failure to validate a world display name.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DisplayNameError {
    /// The canonical value is empty.
    #[error("a world display name must not be empty")]
    Empty,
    /// The value contains a control character, including NUL.
    #[error("a world display name contains forbidden control character U+{codepoint:04X}")]
    ControlCharacter {
        /// Rejected Unicode code point.
        codepoint: u32,
    },
    /// The value contains `/` or `\\`.
    #[error("a world display name must not contain a path separator")]
    PathSeparator,
    /// The scalar count exceeds [`MAX_DISPLAY_NAME_SCALARS`].
    #[error("a world display name has {actual} Unicode scalars; the limit is 128")]
    TooManyScalars {
        /// Observed Unicode scalar count.
        actual: usize,
    },
    /// The encoded length exceeds [`MAX_DISPLAY_NAME_BYTES`].
    #[error("a world display name has {actual} UTF-8 bytes; the limit is 512")]
    TooManyBytes {
        /// Observed UTF-8 byte length.
        actual: usize,
    },
    /// Persistent text was valid but not already trimmed NFC.
    #[error("a persisted world display name must already be trimmed NFC")]
    NonCanonical,
}

/// Opaque index of a canonical, allowlisted world root.
///
/// Root paths are resolved by the host. Keeping only an ordinal in catalog
/// contracts prevents a display name or unchecked path from becoming world
/// identity.
#[repr(transparent)]
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct WorldRootId(pub u32);

/// Structural location of a live world inside an allowlisted root.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LiveWorldLocation {
    /// Allowlisted root containing the world.
    pub root: WorldRootId,
    /// `UUIDv4` encoded by the direct child directory name.
    pub world_id: WorldId,
}

impl LiveWorldLocation {
    /// Creates a path-independent live-world locator.
    #[must_use]
    pub const fn new(root: WorldRootId, world_id: WorldId) -> Self {
        Self { root, world_id }
    }

    /// Returns the only valid direct-child directory name for this location.
    #[must_use]
    pub const fn directory_name(self) -> WorldDirectoryName {
        WorldDirectoryName::for_world(self.world_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD_ID: &str = "123e4567-e89b-42d3-a456-426614174000";

    #[test]
    fn directory_identity_accepts_only_canonical_uuid_v4() {
        let directory = WORLD_ID.parse::<WorldDirectoryName>();
        assert_eq!(
            directory.as_ref().map(ToString::to_string).ok().as_deref(),
            Some(WORLD_ID)
        );
        for rejected in [
            "123E4567-E89B-42D3-A456-426614174000",
            "{123e4567-e89b-42d3-a456-426614174000}",
            "123e4567e89b42d3a456426614174000",
            "123e4567-e89b-12d3-a456-426614174000",
        ] {
            assert!(rejected.parse::<WorldDirectoryName>().is_err());
        }
    }

    #[test]
    fn display_names_are_normalized_without_becoming_paths() {
        let name = DisplayName::new("  Cafe\u{301}  ");
        assert_eq!(
            name.as_ref().map(DisplayName::as_str).ok(),
            Some("Caf\u{e9}")
        );
        assert!(DisplayName::new("bad/name").is_err());
        assert!(DisplayName::new("bad\\name").is_err());
        assert!(DisplayName::new("bad\0name").is_err());
        assert!(DisplayName::parse_canonical(" Cafe ").is_err());
    }
}
