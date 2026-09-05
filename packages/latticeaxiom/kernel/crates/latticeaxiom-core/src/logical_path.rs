use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

/// A canonical, source-root-relative logical path used across stable boundaries.
///
/// The serialized form is UTF-8 NFC text with `/` separators. It is never an
/// operating-system path: absolute paths, Windows drive prefixes, backslashes,
/// control characters, empty segments, and `.` or `..` segments are rejected.
/// The maximum encoded length is 4,096 bytes and each segment is at most 255
/// bytes so untrusted boundary values remain bounded.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CanonicalLogicalPath(String);

impl CanonicalLogicalPath {
    /// Maximum UTF-8 byte length of one canonical logical path.
    pub const MAX_BYTE_LENGTH: usize = 4_096;

    /// Maximum UTF-8 byte length of one canonical path segment.
    pub const MAX_SEGMENT_BYTE_LENGTH: usize = 255;

    /// Creates a validated canonical logical path.
    ///
    /// # Errors
    ///
    /// Returns [`CanonicalLogicalPathError`] when `value` is empty, exceeds a
    /// byte limit, is not root-relative NFC text, uses a host separator or
    /// prefix, contains a control character, or contains an empty or dot
    /// segment.
    pub fn new(value: impl Into<String>) -> Result<Self, CanonicalLogicalPathError> {
        let value = value.into();
        validate(&value)?;
        Ok(Self(value))
    }

    /// Returns the canonical root-relative path text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for CanonicalLogicalPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for CanonicalLogicalPath {
    type Err = CanonicalLogicalPathError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl AsRef<str> for CanonicalLogicalPath {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Serialize for CanonicalLogicalPath {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for CanonicalLogicalPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// An error produced while validating a canonical logical path.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum CanonicalLogicalPathError {
    /// The path was empty.
    #[error("a canonical logical path cannot be empty")]
    Empty,
    /// The full UTF-8 representation exceeded the stable boundary limit.
    #[error("canonical logical path is {actual} bytes; the maximum is {maximum}")]
    PathTooLong {
        /// Actual UTF-8 byte length.
        actual: usize,
        /// Maximum accepted UTF-8 byte length.
        maximum: usize,
    },
    /// The path began with the logical root separator.
    #[error("a canonical logical path must be root-relative")]
    Absolute,
    /// The path began with a Windows drive prefix.
    #[error("a canonical logical path cannot contain a Windows drive prefix")]
    WindowsDrivePrefix,
    /// The path used a host-specific backslash separator.
    #[error("a canonical logical path must use `/` separators")]
    BackslashSeparator,
    /// The path contained a control character.
    #[error("a canonical logical path cannot contain control character U+{code:04X}")]
    ControlCharacter {
        /// Unicode scalar value of the rejected control character.
        code: u32,
    },
    /// The path was not already normalized to Unicode NFC.
    #[error("a canonical logical path must use Unicode NFC normalization")]
    NonNfc,
    /// The path contained an empty segment.
    #[error("a canonical logical path cannot contain an empty segment")]
    EmptySegment,
    /// The path contained `.` or `..` instead of its resolved canonical form.
    #[error("a canonical logical path cannot contain dot segment `{segment}`")]
    DotSegment {
        /// Rejected dot segment.
        segment: &'static str,
    },
    /// One UTF-8 segment exceeded the stable boundary limit.
    #[error("canonical logical path segment is {actual} bytes; the maximum is {maximum}")]
    SegmentTooLong {
        /// Actual UTF-8 byte length.
        actual: usize,
        /// Maximum accepted UTF-8 byte length.
        maximum: usize,
    },
}

fn validate(value: &str) -> Result<(), CanonicalLogicalPathError> {
    if value.is_empty() {
        return Err(CanonicalLogicalPathError::Empty);
    }
    if value.len() > CanonicalLogicalPath::MAX_BYTE_LENGTH {
        return Err(CanonicalLogicalPathError::PathTooLong {
            actual: value.len(),
            maximum: CanonicalLogicalPath::MAX_BYTE_LENGTH,
        });
    }
    if value.starts_with('/') {
        return Err(CanonicalLogicalPathError::Absolute);
    }
    let bytes = value.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(CanonicalLogicalPathError::WindowsDrivePrefix);
    }
    if value.contains('\\') {
        return Err(CanonicalLogicalPathError::BackslashSeparator);
    }
    if let Some(character) = value.chars().find(|character| character.is_control()) {
        return Err(CanonicalLogicalPathError::ControlCharacter {
            code: character as u32,
        });
    }
    if !value.nfc().eq(value.chars()) {
        return Err(CanonicalLogicalPathError::NonNfc);
    }

    for segment in value.split('/') {
        if segment.is_empty() {
            return Err(CanonicalLogicalPathError::EmptySegment);
        }
        if segment == "." {
            return Err(CanonicalLogicalPathError::DotSegment { segment: "." });
        }
        if segment == ".." {
            return Err(CanonicalLogicalPathError::DotSegment { segment: ".." });
        }
        if segment.len() > CanonicalLogicalPath::MAX_SEGMENT_BYTE_LENGTH {
            return Err(CanonicalLogicalPathError::SegmentTooLong {
                actual: segment.len(),
                maximum: CanonicalLogicalPath::MAX_SEGMENT_BYTE_LENGTH,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_root_relative_nfc_paths_and_round_trips() {
        for value in [
            "package.ncl",
            "packages/terrenia/package.ncl",
            "packages/café.ncl",
            "数据/方块.ncl",
        ] {
            let path = CanonicalLogicalPath::new(value)
                .unwrap_or_else(|error| panic!("valid logical path was rejected: {error}"));
            assert_eq!(path.as_str(), value);
            let encoded = serde_json::to_string(&path).unwrap_or_default();
            assert_eq!(
                serde_json::from_str::<CanonicalLogicalPath>(&encoded).ok(),
                Some(path)
            );
        }
    }

    #[test]
    fn rejects_host_separators_prefixes_and_escape_segments() {
        for value in [
            "/package.ncl",
            "C:/package.ncl",
            "c:\\package.ncl",
            "\\\\server\\share\\package.ncl",
            "packages//package.ncl",
            "packages/./package.ncl",
            "packages/../package.ncl",
            "../package.ncl",
            "package.ncl/",
        ] {
            assert!(
                CanonicalLogicalPath::new(value).is_err(),
                "expected `{value}` to be rejected"
            );
        }
    }

    #[test]
    fn rejects_non_nfc_and_control_characters() {
        assert!(CanonicalLogicalPath::new("packages/cafe\u{301}.ncl").is_err());
        assert!(CanonicalLogicalPath::new("packages/line\nfeed.ncl").is_err());
        assert!(CanonicalLogicalPath::new("packages/nul\0byte.ncl").is_err());
    }

    #[test]
    fn enforces_utf8_byte_length_limits_inclusively() {
        let maximum_path = std::iter::repeat_n("a".repeat(240), 17)
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(maximum_path.len(), CanonicalLogicalPath::MAX_BYTE_LENGTH);
        assert!(CanonicalLogicalPath::new(maximum_path.clone()).is_ok());
        assert!(CanonicalLogicalPath::new(format!("{maximum_path}a")).is_err());

        let maximum_segment = "é".repeat(CanonicalLogicalPath::MAX_SEGMENT_BYTE_LENGTH / 2);
        assert_eq!(maximum_segment.len(), 254);
        assert!(CanonicalLogicalPath::new(&maximum_segment).is_ok());
        assert!(CanonicalLogicalPath::new(format!("{maximum_segment}a")).is_ok());
        assert!(CanonicalLogicalPath::new(format!("{maximum_segment}aa")).is_err());
    }

    #[test]
    fn deserialization_requires_a_string_and_revalidates_it() {
        assert!(serde_json::from_str::<CanonicalLogicalPath>(r#""packages/package.ncl""#).is_ok());
        assert!(serde_json::from_str::<CanonicalLogicalPath>(r#""../package.ncl""#).is_err());
        assert!(serde_json::from_str::<CanonicalLogicalPath>(r#"{"path":"package.ncl"}"#).is_err());
    }
}
