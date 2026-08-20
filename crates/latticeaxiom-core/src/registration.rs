use std::{fmt, str::FromStr};

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::{StableId, grammar::is_canonical_identifier_segment};

/// A registration namespace governed independently from package names.
///
/// The grammar is one lowercase ASCII identifier segment. Letters, digits,
/// `.`, `_`, and `-` are accepted, and the first and last characters must be
/// a letter or digit. The UTF-8 representation is bounded to 255 bytes.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RegistrationNamespace(String);

impl RegistrationNamespace {
    /// Maximum byte length of a registration namespace.
    pub const MAX_BYTE_LENGTH: usize = 255;

    /// Creates a validated registration namespace.
    ///
    /// # Errors
    ///
    /// Returns [`RegistrationNamespaceError`] when `value` is empty, too long,
    /// or violates the canonical lowercase ASCII segment grammar.
    pub fn new(value: impl Into<String>) -> Result<Self, RegistrationNamespaceError> {
        let value = value.into();
        validate_namespace(&value)?;
        Ok(Self(value))
    }

    /// Returns the canonical namespace text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns whether a stable identifier belongs to this namespace.
    #[must_use]
    pub fn contains(&self, id: &StableId) -> bool {
        self.as_str() == id.namespace()
    }
}

impl fmt::Display for RegistrationNamespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for RegistrationNamespace {
    type Err = RegistrationNamespaceError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl AsRef<str> for RegistrationNamespace {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Serialize for RegistrationNamespace {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for RegistrationNamespace {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// An error produced while validating a registration namespace.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum RegistrationNamespaceError {
    /// The namespace was empty.
    #[error("a registration namespace cannot be empty")]
    Empty,
    /// The namespace exceeded the stable boundary limit.
    #[error("registration namespace is {actual} bytes; the maximum is {maximum}")]
    TooLong {
        /// Actual byte length.
        actual: usize,
        /// Maximum accepted byte length.
        maximum: usize,
    },
    /// The namespace did not use the canonical lowercase ASCII grammar.
    #[error(
        "a registration namespace must be a lowercase ASCII segment that begins and ends with a letter or digit"
    )]
    InvalidGrammar,
}

/// A bounded permission pattern for full stable registration identifiers.
///
/// The grammar is `<namespace>:<kind>/<path-pattern>`. Namespace, kind, and
/// literal path segments use the same canonical lowercase ASCII grammar as
/// [`StableId`]. A path segment may instead be `*`, matching exactly one
/// segment, or a terminal `**`, matching zero or more segments. Contract-major
/// suffixes are deliberately absent because a grant governs registration
/// ownership independently of a particular contract major.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NamespaceGrantPattern {
    value: String,
    namespace_end: usize,
    kind_end: usize,
}

impl NamespaceGrantPattern {
    /// Maximum UTF-8 byte length of a complete namespace grant pattern.
    pub const MAX_BYTE_LENGTH: usize = 4_096;

    /// Maximum byte length of each kind or literal path segment.
    pub const MAX_SEGMENT_BYTE_LENGTH: usize = 255;

    /// Creates a validated namespace grant pattern.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceGrantPatternError`] when `value` is empty, exceeds a
    /// byte limit, is not NFC, does not contain one namespace and kind, uses a
    /// contract-major suffix, or contains an invalid or ambiguous wildcard.
    pub fn new(value: impl Into<String>) -> Result<Self, NamespaceGrantPatternError> {
        let value = value.into();
        let (namespace_end, kind_end) = validate_pattern(&value)?;
        Ok(Self {
            value,
            namespace_end,
            kind_end,
        })
    }

    /// Returns the complete canonical pattern text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Returns the governed registration namespace.
    #[must_use]
    pub fn namespace(&self) -> &str {
        &self.value[..self.namespace_end]
    }

    /// Returns the governed registration kind.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.value[self.namespace_end + 1..self.kind_end]
    }

    /// Returns the slash-separated path pattern.
    #[must_use]
    pub fn path_pattern(&self) -> &str {
        &self.value[self.kind_end + 1..]
    }

    /// Returns whether this permission pattern covers a stable identifier.
    ///
    /// A terminal `**` matches zero or more complete path segments, while `*`
    /// matches exactly one. The stable identifier's optional contract major is
    /// ignored because it is outside the path.
    #[must_use]
    pub fn matches(&self, id: &StableId) -> bool {
        if self.namespace() != id.namespace() || self.kind() != id.kind() {
            return false;
        }

        let mut path_segments = id.path().split('/');
        for pattern_segment in self.path_pattern().split('/') {
            match pattern_segment {
                "**" => return true,
                "*" => {
                    if path_segments.next().is_none() {
                        return false;
                    }
                }
                literal => {
                    if path_segments.next() != Some(literal) {
                        return false;
                    }
                }
            }
        }
        path_segments.next().is_none()
    }
}

impl fmt::Display for NamespaceGrantPattern {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for NamespaceGrantPattern {
    type Err = NamespaceGrantPatternError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

impl AsRef<str> for NamespaceGrantPattern {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Serialize for NamespaceGrantPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for NamespaceGrantPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(de::Error::custom)
    }
}

/// An error produced while validating a namespace grant pattern.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum NamespaceGrantPatternError {
    /// The pattern was empty.
    #[error("a namespace grant pattern cannot be empty")]
    Empty,
    /// The complete pattern exceeded the stable boundary limit.
    #[error("namespace grant pattern is {actual} bytes; the maximum is {maximum}")]
    TooLong {
        /// Actual byte length.
        actual: usize,
        /// Maximum accepted byte length.
        maximum: usize,
    },
    /// The pattern was not already normalized to Unicode NFC.
    #[error("a namespace grant pattern must use Unicode NFC normalization")]
    NonNfc,
    /// The pattern did not contain exactly one namespace separator.
    #[error("a namespace grant pattern requires exactly one `:` namespace separator")]
    InvalidNamespaceSeparator,
    /// The namespace component was invalid.
    #[error("invalid namespace in namespace grant pattern: {source}")]
    InvalidNamespace {
        /// Underlying namespace validation failure.
        source: RegistrationNamespaceError,
    },
    /// The pattern omitted its kind or path separator.
    #[error("a namespace grant pattern requires a kind and path separated by `/`")]
    MissingPath,
    /// The kind exceeded the stable boundary limit.
    #[error("namespace grant kind is {actual} bytes; the maximum is {maximum}")]
    KindTooLong {
        /// Actual kind byte length.
        actual: usize,
        /// Maximum accepted kind byte length.
        maximum: usize,
    },
    /// The kind did not use the canonical lowercase ASCII grammar.
    #[error("namespace grant kind must be a canonical lowercase ASCII segment")]
    InvalidKind,
    /// A contract-major suffix appeared in a permission pattern.
    #[error("a namespace grant pattern cannot contain a contract-major suffix")]
    ContractMajorNotAllowed,
    /// The pattern contained an empty path segment.
    #[error("a namespace grant pattern cannot contain an empty path segment")]
    EmptyPathSegment,
    /// The pattern contained a `.` or `..` path segment.
    #[error("a namespace grant pattern cannot contain dot segment `{segment}`")]
    DotSegment {
        /// Rejected dot segment.
        segment: &'static str,
    },
    /// A literal path segment exceeded the stable boundary limit.
    #[error("namespace grant path segment is {actual} bytes; the maximum is {maximum}")]
    PathSegmentTooLong {
        /// Actual segment byte length.
        actual: usize,
        /// Maximum accepted segment byte length.
        maximum: usize,
    },
    /// A literal path segment did not use the canonical lowercase ASCII grammar.
    #[error("namespace grant literal path segments must use canonical lowercase ASCII grammar")]
    InvalidLiteral,
    /// `*` appeared inside a literal instead of as a complete segment.
    #[error("namespace grant wildcards must occupy a complete path segment")]
    EmbeddedWildcard,
    /// Recursive `**` appeared before the final path segment.
    #[error("recursive namespace grant wildcard `**` must be the final path segment")]
    NonTerminalRecursiveWildcard,
}

fn validate_namespace(value: &str) -> Result<(), RegistrationNamespaceError> {
    if value.is_empty() {
        return Err(RegistrationNamespaceError::Empty);
    }
    if value.len() > RegistrationNamespace::MAX_BYTE_LENGTH {
        return Err(RegistrationNamespaceError::TooLong {
            actual: value.len(),
            maximum: RegistrationNamespace::MAX_BYTE_LENGTH,
        });
    }
    if !is_canonical_identifier_segment(value) {
        return Err(RegistrationNamespaceError::InvalidGrammar);
    }
    Ok(())
}

fn validate_pattern(value: &str) -> Result<(usize, usize), NamespaceGrantPatternError> {
    if value.is_empty() {
        return Err(NamespaceGrantPatternError::Empty);
    }
    if value.len() > NamespaceGrantPattern::MAX_BYTE_LENGTH {
        return Err(NamespaceGrantPatternError::TooLong {
            actual: value.len(),
            maximum: NamespaceGrantPattern::MAX_BYTE_LENGTH,
        });
    }
    if !value.nfc().eq(value.chars()) {
        return Err(NamespaceGrantPatternError::NonNfc);
    }

    let Some((namespace, remainder)) = value.split_once(':') else {
        return Err(NamespaceGrantPatternError::InvalidNamespaceSeparator);
    };
    if remainder.contains(':') {
        return Err(NamespaceGrantPatternError::InvalidNamespaceSeparator);
    }
    validate_namespace(namespace)
        .map_err(|source| NamespaceGrantPatternError::InvalidNamespace { source })?;

    let Some((kind, path_pattern)) = remainder.split_once('/') else {
        return Err(NamespaceGrantPatternError::MissingPath);
    };
    if kind.len() > NamespaceGrantPattern::MAX_SEGMENT_BYTE_LENGTH {
        return Err(NamespaceGrantPatternError::KindTooLong {
            actual: kind.len(),
            maximum: NamespaceGrantPattern::MAX_SEGMENT_BYTE_LENGTH,
        });
    }
    if !is_canonical_identifier_segment(kind) {
        return Err(NamespaceGrantPatternError::InvalidKind);
    }
    if value.contains('@') {
        return Err(NamespaceGrantPatternError::ContractMajorNotAllowed);
    }

    let mut segments = path_pattern.split('/').peekable();
    while let Some(segment) = segments.next() {
        if segment.is_empty() {
            return Err(NamespaceGrantPatternError::EmptyPathSegment);
        }
        if segment == "." {
            return Err(NamespaceGrantPatternError::DotSegment { segment: "." });
        }
        if segment == ".." {
            return Err(NamespaceGrantPatternError::DotSegment { segment: ".." });
        }
        if segment == "**" {
            if segments.peek().is_some() {
                return Err(NamespaceGrantPatternError::NonTerminalRecursiveWildcard);
            }
            continue;
        }
        if segment == "*" {
            continue;
        }
        if segment.contains('*') {
            return Err(NamespaceGrantPatternError::EmbeddedWildcard);
        }
        if segment.len() > NamespaceGrantPattern::MAX_SEGMENT_BYTE_LENGTH {
            return Err(NamespaceGrantPatternError::PathSegmentTooLong {
                actual: segment.len(),
                maximum: NamespaceGrantPattern::MAX_SEGMENT_BYTE_LENGTH,
            });
        }
        if !is_canonical_identifier_segment(segment) {
            return Err(NamespaceGrantPatternError::InvalidLiteral);
        }
    }

    let namespace_end = namespace.len();
    let kind_end = namespace_end + 1 + kind.len();
    Ok((namespace_end, kind_end))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registration_namespaces_use_an_independent_strict_grammar() {
        for value in ["terrenia", "latticeaxiom", "example-2", "a.b_c"] {
            let namespace = RegistrationNamespace::new(value)
                .unwrap_or_else(|error| panic!("valid namespace was rejected: {error}"));
            assert_eq!(namespace.as_str(), value);
        }

        for value in [
            "",
            "Terrenia",
            "-terrenia",
            "terrenia-",
            "@terrenia/blocks",
            "terrenia:block",
            "terrenia/block",
            "terrénia",
            "terrenia*",
        ] {
            assert!(
                RegistrationNamespace::new(value).is_err(),
                "expected `{value}` to be rejected"
            );
        }
    }

    #[test]
    fn registration_namespace_length_is_bounded_and_serde_revalidates() {
        let maximum = "a".repeat(RegistrationNamespace::MAX_BYTE_LENGTH);
        assert!(RegistrationNamespace::new(&maximum).is_ok());
        assert!(RegistrationNamespace::new(format!("{maximum}a")).is_err());
        assert!(serde_json::from_str::<RegistrationNamespace>(r#""terrenia""#).is_ok());
        assert!(serde_json::from_str::<RegistrationNamespace>(r#""Terrenia""#).is_err());
        assert!(serde_json::from_str::<RegistrationNamespace>(r#"{"name":"terrenia"}"#).is_err());
    }

    #[test]
    fn grant_patterns_accept_documented_full_id_forms() {
        for value in [
            "terrenia:block/**",
            "terrenia:block/*",
            "terrenia:block/stone",
            "terrenia:block/storage/*",
            "terrenia:block/storage/**",
            "terrenia:block-role/**",
        ] {
            let pattern = NamespaceGrantPattern::new(value)
                .unwrap_or_else(|error| panic!("valid grant pattern was rejected: {error}"));
            assert_eq!(pattern.as_str(), value);
            assert_eq!(pattern.namespace(), "terrenia");
        }
    }

    #[test]
    fn grant_patterns_reject_ambiguous_wildcards_and_escape_forms() {
        for value in [
            "",
            "block/**",
            "terrenia:block",
            "terrenia::block/**",
            "Terrenia:block/**",
            "terrenia:Block/**",
            "terrenia:block/",
            "terrenia:block//stone",
            "terrenia:block/./stone",
            "terrenia:block/../stone",
            "terrenia:block/stone\\variant",
            "terrenia:block/stone@1",
            "terrenia:block/sto*ne",
            "terrenia:block/**/stone",
            "terrenia:block/***",
            "terrenia:block/cafe\u{301}",
        ] {
            assert!(
                NamespaceGrantPattern::new(value).is_err(),
                "expected `{value}` to be rejected"
            );
        }
    }

    #[test]
    fn grant_patterns_match_only_complete_stable_id_segments() {
        let recursive = pattern("terrenia:block/storage/**");
        assert!(recursive.matches(&stable_id("terrenia:block/storage")));
        assert!(recursive.matches(&stable_id("terrenia:block/storage/copper")));
        assert!(recursive.matches(&stable_id("terrenia:block/storage/copper@2")));
        assert!(!recursive.matches(&stable_id("terrenia:block/storage-crate")));
        assert!(!recursive.matches(&stable_id("example:block/storage/copper")));
        assert!(!recursive.matches(&stable_id("terrenia:item/storage/copper")));

        let single = pattern("terrenia:block/*");
        assert!(single.matches(&stable_id("terrenia:block/stone")));
        assert!(!single.matches(&stable_id("terrenia:block/natural/stone")));

        let exact = pattern("terrenia:block/natural/stone");
        assert!(exact.matches(&stable_id("terrenia:block/natural/stone@1")));
        assert!(!exact.matches(&stable_id("terrenia:block/natural/dirt")));
    }

    #[test]
    fn grant_pattern_lengths_are_bounded_in_bytes() {
        let literal = "a".repeat(NamespaceGrantPattern::MAX_SEGMENT_BYTE_LENGTH);
        assert!(NamespaceGrantPattern::new(format!("a:b/{literal}")).is_ok());
        assert!(NamespaceGrantPattern::new(format!("a:b/{literal}a")).is_err());

        let mut path_segments = std::iter::repeat_n("a".repeat(240), 16).collect::<Vec<_>>();
        path_segments.push("a".repeat(236));
        let maximum = format!("a:b/{}", path_segments.join("/"));
        assert_eq!(maximum.len(), NamespaceGrantPattern::MAX_BYTE_LENGTH);
        assert!(NamespaceGrantPattern::new(&maximum).is_ok());
        assert!(NamespaceGrantPattern::new(format!("{maximum}a")).is_err());
    }

    #[test]
    fn grant_pattern_serde_is_a_strict_revalidating_string_boundary() {
        let decoded = serde_json::from_str::<NamespaceGrantPattern>(r#""terrenia:block/**""#)
            .unwrap_or_else(|error| panic!("valid pattern JSON was rejected: {error}"));
        assert_eq!(decoded, pattern("terrenia:block/**"));
        assert!(
            serde_json::from_str::<NamespaceGrantPattern>(r#""terrenia:block/**/stone""#).is_err()
        );
        assert!(
            serde_json::from_str::<NamespaceGrantPattern>(r#"{"pattern":"terrenia:block/**"}"#)
                .is_err()
        );
    }

    fn pattern(value: &str) -> NamespaceGrantPattern {
        NamespaceGrantPattern::new(value)
            .unwrap_or_else(|error| panic!("test grant pattern was invalid: {error}"))
    }

    fn stable_id(value: &str) -> StableId {
        value
            .parse()
            .unwrap_or_else(|error| panic!("test stable ID was invalid: {error}"))
    }
}
