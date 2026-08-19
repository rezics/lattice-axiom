use serde::{Deserialize, Deserializer, Serialize, de};
use thiserror::Error;
use unicode_normalization::UnicodeNormalization;

use crate::canonical::{CanonicalHash, CanonicalJsonError, canonical_json_hash};
use crate::identifier::SourceId;

/// A half-open byte range in one authored source file.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceSpan {
    start_byte: u64,
    end_byte: u64,
}

impl SourceSpan {
    /// Creates a validated half-open source span.
    ///
    /// # Errors
    ///
    /// Returns [`SourceProvenanceError::ReversedSpan`] when `end_byte` is less
    /// than `start_byte`.
    pub const fn new(start_byte: u64, end_byte: u64) -> Result<Self, SourceProvenanceError> {
        if end_byte < start_byte {
            return Err(SourceProvenanceError::ReversedSpan {
                start_byte,
                end_byte,
            });
        }
        Ok(Self {
            start_byte,
            end_byte,
        })
    }

    /// Returns the inclusive start byte offset.
    #[must_use]
    pub const fn start_byte(self) -> u64 {
        self.start_byte
    }

    /// Returns the exclusive end byte offset.
    #[must_use]
    pub const fn end_byte(self) -> u64 {
        self.end_byte
    }

    /// Returns the number of bytes covered by this span.
    #[must_use]
    pub const fn len(self) -> u64 {
        self.end_byte - self.start_byte
    }

    /// Returns whether this span contains no bytes.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.start_byte == self.end_byte
    }
}

impl<'de> Deserialize<'de> for SourceSpan {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            start_byte: u64,
            end_byte: u64,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(fields.start_byte, fields.end_byte).map_err(de::Error::custom)
    }
}

/// The operation that introduced one step in authored source provenance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SourceOriginKind {
    /// A Nickel import introduced the value.
    Import,
    /// A named constructor or helper produced the value.
    Helper,
    /// A profile or package overlay replaced or extended the value.
    Overlay,
    /// A generated manifest or schema fragment produced the value.
    Generated,
}

/// One ordered import, helper, overlay, or generation step.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceOrigin {
    kind: SourceOriginKind,
    identity: String,
}

impl SourceOrigin {
    /// Creates a source origin step with a non-empty stable identity.
    ///
    /// # Errors
    ///
    /// Returns [`SourceProvenanceError::EmptyOriginIdentity`] when `identity`
    /// is empty.
    pub fn new(
        kind: SourceOriginKind,
        identity: impl Into<String>,
    ) -> Result<Self, SourceProvenanceError> {
        let identity = identity.into();
        if identity.is_empty() {
            return Err(SourceProvenanceError::EmptyOriginIdentity { kind });
        }
        Ok(Self { kind, identity })
    }

    /// Returns the kind of provenance step.
    #[must_use]
    pub const fn kind(&self) -> SourceOriginKind {
        self.kind
    }

    /// Returns the stable identity of the source operation.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }
}

impl<'de> Deserialize<'de> for SourceOrigin {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            kind: SourceOriginKind,
            identity: String,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(fields.kind, fields.identity).map_err(de::Error::custom)
    }
}

/// Diagnostic and reproducibility provenance for one authored contribution.
///
/// This value carries a stable source ID, canonical logical path, raw content
/// hash, optional byte span, and ordered origin chain. Optional generation
/// metadata identifies the producer, toolchain, and generated fragment. None
/// of these fields enter a semantic hash unless the caller explicitly includes
/// this value; use [`provenance_hash`] for the independent provenance digest.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceProvenance {
    source_id: SourceId,
    logical_path: String,
    content_hash: CanonicalHash,
    span: Option<SourceSpan>,
    origin_chain: Vec<SourceOrigin>,
    producer: Option<String>,
    toolchain: Option<String>,
    generated_fragment: Option<String>,
}

impl SourceProvenance {
    /// Creates validated source provenance.
    ///
    /// # Errors
    ///
    /// Returns [`SourceProvenanceError::InvalidLogicalPath`] when the path is
    /// empty, absolute, backslash-separated, or contains non-canonical dot or
    /// empty segments.
    pub fn new(
        source_id: SourceId,
        logical_path: impl Into<String>,
        content_hash: CanonicalHash,
        span: Option<SourceSpan>,
        origin_chain: Vec<SourceOrigin>,
    ) -> Result<Self, SourceProvenanceError> {
        let logical_path = logical_path.into();
        validate_logical_path(&logical_path)?;
        Ok(Self {
            source_id,
            logical_path,
            content_hash,
            span,
            origin_chain,
            producer: None,
            toolchain: None,
            generated_fragment: None,
        })
    }

    /// Adds optional producer, toolchain, and generated-fragment identities.
    ///
    /// # Errors
    ///
    /// Returns [`SourceProvenanceError::EmptyGenerationIdentity`] when any
    /// provided identity is empty.
    pub fn with_generation(
        mut self,
        producer: Option<String>,
        toolchain: Option<String>,
        generated_fragment: Option<String>,
    ) -> Result<Self, SourceProvenanceError> {
        validate_optional_identity("producer", producer.as_deref())?;
        validate_optional_identity("toolchain", toolchain.as_deref())?;
        validate_optional_identity("generated_fragment", generated_fragment.as_deref())?;
        self.producer = producer;
        self.toolchain = toolchain;
        self.generated_fragment = generated_fragment;
        Ok(self)
    }

    /// Returns the stable package-source identity.
    #[must_use]
    pub const fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Returns the canonical root-relative logical path.
    #[must_use]
    pub fn logical_path(&self) -> &str {
        &self.logical_path
    }

    /// Returns the raw file-content SHA-256 digest.
    #[must_use]
    pub const fn content_hash(&self) -> CanonicalHash {
        self.content_hash
    }

    /// Returns the optional byte span within the logical source file.
    #[must_use]
    pub const fn span(&self) -> Option<SourceSpan> {
        self.span
    }

    /// Returns the ordered source operation chain.
    #[must_use]
    pub fn origin_chain(&self) -> &[SourceOrigin] {
        &self.origin_chain
    }

    /// Returns the optional producer identity.
    #[must_use]
    pub fn producer(&self) -> Option<&str> {
        self.producer.as_deref()
    }

    /// Returns the optional toolchain identity.
    #[must_use]
    pub fn toolchain(&self) -> Option<&str> {
        self.toolchain.as_deref()
    }

    /// Returns the optional generated fragment identity.
    #[must_use]
    pub fn generated_fragment(&self) -> Option<&str> {
        self.generated_fragment.as_deref()
    }
}

impl<'de> Deserialize<'de> for SourceProvenance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Fields {
            source_id: SourceId,
            logical_path: String,
            content_hash: CanonicalHash,
            span: Option<SourceSpan>,
            origin_chain: Vec<SourceOrigin>,
            producer: Option<String>,
            toolchain: Option<String>,
            generated_fragment: Option<String>,
        }

        let fields = Fields::deserialize(deserializer)?;
        Self::new(
            fields.source_id,
            fields.logical_path,
            fields.content_hash,
            fields.span,
            fields.origin_chain,
        )
        .and_then(|value| {
            value.with_generation(fields.producer, fields.toolchain, fields.generated_fragment)
        })
        .map_err(de::Error::custom)
    }
}

/// Computes the independent canonical provenance digest.
///
/// This hash must not be used as a semantic compatibility hash.
///
/// # Errors
///
/// Returns an error if provenance cannot be represented as canonical JSON.
pub fn provenance_hash(provenance: &SourceProvenance) -> Result<CanonicalHash, CanonicalJsonError> {
    canonical_json_hash(provenance)
}

/// An error produced while validating source provenance.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum SourceProvenanceError {
    /// A source span ended before it started.
    #[error("source span end byte {end_byte} precedes start byte {start_byte}")]
    ReversedSpan {
        /// The inclusive start byte offset.
        start_byte: u64,
        /// The exclusive end byte offset.
        end_byte: u64,
    },
    /// A logical source path was not canonical and root-relative.
    #[error("invalid logical source path `{value}`: {reason}")]
    InvalidLogicalPath {
        /// Rejected logical path.
        value: String,
        /// Violated canonical-path rule.
        reason: &'static str,
    },
    /// An origin-chain step had no stable identity.
    #[error("{kind:?} origin-chain step requires a non-empty identity")]
    EmptyOriginIdentity {
        /// The origin kind whose identity was empty.
        kind: SourceOriginKind,
    },
    /// Optional generation metadata contained an empty identity.
    #[error("generation provenance field `{field}` cannot be empty")]
    EmptyGenerationIdentity {
        /// The rejected generation metadata field.
        field: &'static str,
    },
}

fn validate_optional_identity(
    field: &'static str,
    identity: Option<&str>,
) -> Result<(), SourceProvenanceError> {
    if identity == Some("") {
        return Err(SourceProvenanceError::EmptyGenerationIdentity { field });
    }
    Ok(())
}

fn validate_logical_path(path: &str) -> Result<(), SourceProvenanceError> {
    let invalid = |reason| SourceProvenanceError::InvalidLogicalPath {
        value: path.to_owned(),
        reason,
    };
    if path.is_empty() {
        return Err(invalid("the path cannot be empty"));
    }
    if path.starts_with('/') {
        return Err(invalid("the path must be root-relative"));
    }
    let bytes = path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return Err(invalid("the path cannot contain a Windows drive prefix"));
    }
    if path.contains('\\') {
        return Err(invalid("the path must use `/` separators"));
    }
    if path.contains('\0') {
        return Err(invalid("the path cannot contain a NUL byte"));
    }
    if !path.nfc().eq(path.chars()) {
        return Err(invalid("the path must use Unicode NFC normalization"));
    }
    if path
        .split('/')
        .any(|segment| segment.is_empty() || matches!(segment, "." | ".."))
    {
        return Err(invalid("the path contains an empty or dot segment"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_span_enforces_ordering_and_strict_fields() {
        let span = SourceSpan::new(10, 20)
            .unwrap_or_else(|error| panic!("valid source span was rejected: {error}"));
        assert_eq!(span.start_byte(), 10);
        assert_eq!(span.end_byte(), 20);
        assert_eq!(span.len(), 10);
        assert!(!span.is_empty());
        assert!(SourceSpan::new(20, 10).is_err());
        assert!(SourceSpan::new(20, 20).is_ok());
        assert!(
            serde_json::from_str::<SourceSpan>(r#"{"start_byte":10,"end_byte":20,"unknown":true}"#)
                .is_err()
        );
    }

    #[test]
    fn provenance_round_trips_with_source_id_and_origin_chain() {
        let provenance = fixture_provenance("latticeaxiom:source/terrenia", "blocks/package.ncl")
            .with_generation(
                Some("latticeaxiom-compose@0.1.0".to_owned()),
                Some("rustc-1.97.1".to_owned()),
                Some("registration:block-definitions".to_owned()),
            )
            .unwrap_or_else(|error| panic!("valid generation provenance was rejected: {error}"));
        let encoded = serde_json::to_string(&provenance).unwrap_or_default();
        assert_eq!(
            serde_json::from_str::<SourceProvenance>(&encoded).ok(),
            Some(provenance)
        );
    }

    #[test]
    fn provenance_deserialization_denies_unknown_fields() {
        let hash = CanonicalHash::digest(b"source");
        let value = format!(
            r#"{{"source_id":"latticeaxiom:source/a","logical_path":"a.ncl","content_hash":"{hash}","span":null,"origin_chain":[],"producer":null,"toolchain":null,"generated_fragment":null,"unknown":true}}"#
        );
        assert!(serde_json::from_str::<SourceProvenance>(&value).is_err());
        assert!(
            serde_json::from_str::<SourceOrigin>(
                r#"{"kind":"import","identity":"a.ncl","unknown":true}"#
            )
            .is_err()
        );
    }

    #[test]
    fn provenance_rejects_noncanonical_logical_paths() {
        for path in [
            "",
            "/package.ncl",
            "C:/package.ncl",
            "packages\\package.ncl",
            "packages/../package.ncl",
            "packages/./package.ncl",
            "packages//package.ncl",
            "packages/cafe\u{301}.ncl",
        ] {
            let result = SourceProvenance::new(
                "latticeaxiom:source/test"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
                path,
                CanonicalHash::digest(b"source"),
                None,
                Vec::new(),
            );
            assert!(result.is_err(), "expected `{path}` to be rejected");
        }

        assert!(
            SourceProvenance::new(
                "latticeaxiom:source/test"
                    .parse()
                    .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
                "packages/café.ncl",
                CanonicalHash::digest(b"source"),
                None,
                Vec::new(),
            )
            .is_ok()
        );
    }

    #[test]
    fn provenance_hash_changes_independently_from_semantic_hash() {
        let first = fixture_provenance("latticeaxiom:source/a", "package.ncl");
        let second = fixture_provenance("latticeaxiom:source/b", "moved/package.ncl");
        assert_ne!(provenance_hash(&first).ok(), provenance_hash(&second).ok());

        let semantic_payload = ["terrenia:block/stone", "terrenia:block/dirt"];
        let semantic_hash = canonical_json_hash(&semantic_payload).ok();
        assert_eq!(semantic_hash, canonical_json_hash(&semantic_payload).ok());
    }

    fn fixture_provenance(source_id: &str, logical_path: &str) -> SourceProvenance {
        let origin = SourceOrigin::new(SourceOriginKind::Import, "root/game.ncl")
            .unwrap_or_else(|error| panic!("valid source origin was rejected: {error}"));
        SourceProvenance::new(
            source_id
                .parse()
                .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
            logical_path,
            CanonicalHash::digest(b"source"),
            Some(
                SourceSpan::new(3, 9)
                    .unwrap_or_else(|error| panic!("valid source span was rejected: {error}")),
            ),
            vec![origin],
        )
        .unwrap_or_else(|error| panic!("valid provenance was rejected: {error}"))
    }
}
