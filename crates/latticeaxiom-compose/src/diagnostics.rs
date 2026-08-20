//! Stable diagnostics that cross composition and persistence boundaries.

use std::{cmp::Ordering, collections::BTreeMap, fmt, str::FromStr};

use latticeaxiom_core::{
    CanonicalJsonError, IdentifierError, SourceProvenance, StableId, canonical_json_bytes,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Current version of the composition diagnostic-code catalog.
pub const DIAGNOSTIC_CATALOG_VERSION: u32 = 2;

/// A stable, owner-scoped machine-readable diagnostic code.
///
/// Codes contain two or more dot-separated lowercase segments, such as
/// `compose.output_limit` or `semantic.role_ambiguous`.
#[repr(transparent)]
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DiagnosticCode(String);

impl DiagnosticCode {
    /// Returns the canonical diagnostic code text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[cfg(feature = "nickel-evaluator")]
    pub(crate) fn from_builtin(value: &'static str) -> Self {
        Self(value.to_owned())
    }
}

impl fmt::Display for DiagnosticCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DiagnosticCode {
    type Err = DiagnosticCodeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let segments = value.split('.').collect::<Vec<_>>();
        let valid = segments.len() >= 2
            && segments.iter().all(|segment| {
                segment
                    .bytes()
                    .next()
                    .is_some_and(|byte| byte.is_ascii_lowercase())
                    && segment.bytes().all(|byte| {
                        byte.is_ascii_lowercase()
                            || byte.is_ascii_digit()
                            || matches!(byte, b'_' | b'-')
                    })
            });
        if valid {
            Ok(Self(value.to_owned()))
        } else {
            Err(DiagnosticCodeError::InvalidCode {
                value: value.to_owned(),
            })
        }
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for DiagnosticCode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
    }
}

/// User-facing importance of one stable diagnostic.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticSeverity {
    /// Evaluation or activation cannot continue.
    Error,
    /// Evaluation may continue, but author intent is suspicious or deprecated.
    Warning,
    /// Informational resolution or recovery context.
    Info,
}

/// One source-aware label attached to a diagnostic.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticLabel {
    /// Trusted source provenance for the labeled contribution.
    pub provenance: SourceProvenance,
    /// Concise explanation of this source location.
    pub message: String,
}

/// A stable diagnostic DTO suitable for CLI, UI, and persisted reports.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    /// Machine-readable code from a versioned catalog.
    pub code: DiagnosticCode,
    /// Diagnostic severity.
    pub severity: DiagnosticSeverity,
    /// Concise user-facing summary without terminal formatting.
    pub summary: String,
    /// Ordered source labels, with the primary location first.
    pub labels: Vec<DiagnosticLabel>,
    /// Ordered actionable context and remediation notes.
    pub notes: Vec<String>,
}

/// One owned entry in the versioned diagnostic catalog.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DiagnosticCatalogEntry {
    /// Stable diagnostic code.
    pub code: DiagnosticCode,
    /// Owner of code semantics and compatibility.
    pub owner: StableId,
    /// Default severity before a presentation surface applies policy.
    pub default_severity: DiagnosticSeverity,
}

#[derive(Debug)]
struct CanonicalDiagnostic {
    diagnostic: Diagnostic,
    canonical_labels: Vec<u8>,
    canonical_notes: Vec<u8>,
    canonical_dto: Vec<u8>,
}

impl CanonicalDiagnostic {
    fn new(diagnostic: Diagnostic) -> Result<Self, CanonicalJsonError> {
        let canonical_labels = canonical_json_bytes(&diagnostic.labels)?;
        let canonical_notes = canonical_json_bytes(&diagnostic.notes)?;
        let canonical_dto = canonical_json_bytes(&diagnostic)?;
        Ok(Self {
            diagnostic,
            canonical_labels,
            canonical_notes,
            canonical_dto,
        })
    }
}

/// Sorts, deduplicates, and bounds a collection of stable diagnostics.
///
/// Sorting follows the ADR 0022 bytewise key: severity, code, primary source
/// identity/path/span, summary, canonical labels, and canonical notes. Only
/// diagnostics with identical complete canonical DTO bytes are deduplicated.
/// When the unique count exceeds `limit`, this function retains the first
/// `limit - 1` diagnostics and appends `compose.diagnostics_truncated`.
///
/// # Errors
///
/// Returns [`DiagnosticPolicyError::ZeroLimit`] when `limit` is zero. Returns
/// [`DiagnosticPolicyError::CanonicalEncoding`] if a diagnostic cannot be
/// encoded as canonical JSON for deterministic ordering and deduplication.
pub fn normalize_diagnostics(
    diagnostics: Vec<Diagnostic>,
    limit: usize,
) -> Result<Vec<Diagnostic>, DiagnosticPolicyError> {
    if limit == 0 {
        return Err(DiagnosticPolicyError::ZeroLimit);
    }

    let mut diagnostics = diagnostics
        .into_iter()
        .map(CanonicalDiagnostic::new)
        .collect::<Result<Vec<_>, _>>()?;
    diagnostics.sort_by(compare_diagnostics);
    diagnostics.dedup_by(|left, right| left.canonical_dto == right.canonical_dto);

    if diagnostics.len() > limit {
        let total = diagnostics.len();
        let retained = limit - 1;
        let omitted = total - retained;
        diagnostics.truncate(retained);
        diagnostics.push(CanonicalDiagnostic::new(truncation_diagnostic(
            total, retained, omitted,
        ))?);
    }

    Ok(diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.diagnostic)
        .collect())
}

fn compare_diagnostics(left: &CanonicalDiagnostic, right: &CanonicalDiagnostic) -> Ordering {
    let left_primary = primary_location(&left.diagnostic);
    let right_primary = primary_location(&right.diagnostic);

    severity_rank(left.diagnostic.severity)
        .cmp(&severity_rank(right.diagnostic.severity))
        .then_with(|| {
            left.diagnostic
                .code
                .as_str()
                .cmp(right.diagnostic.code.as_str())
        })
        .then_with(|| left_primary.0.cmp(right_primary.0))
        .then_with(|| left_primary.1.cmp(right_primary.1))
        .then_with(|| left_primary.2.cmp(&right_primary.2))
        .then_with(|| left_primary.3.cmp(&right_primary.3))
        .then_with(|| left.diagnostic.summary.cmp(&right.diagnostic.summary))
        .then_with(|| left.canonical_labels.cmp(&right.canonical_labels))
        .then_with(|| left.canonical_notes.cmp(&right.canonical_notes))
}

fn severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
    }
}

fn primary_location(diagnostic: &Diagnostic) -> (&str, &str, u64, u64) {
    diagnostic.labels.first().map_or(("", "", 0, 0), |label| {
        let span = label.provenance.span();
        (
            label.provenance.source_id().as_str(),
            label.provenance.logical_path(),
            span.map_or(0, latticeaxiom_core::SourceSpan::start_byte),
            span.map_or(0, latticeaxiom_core::SourceSpan::end_byte),
        )
    })
}

fn truncation_diagnostic(total: usize, retained: usize, omitted: usize) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode("compose.diagnostics_truncated".to_owned()),
        severity: DiagnosticSeverity::Warning,
        summary: "diagnostic output was truncated".to_owned(),
        labels: Vec::new(),
        notes: vec![format!(
            "total={total}; retained={retained}; omitted={omitted}"
        )],
    }
}

/// Returns the minimum R0 composition and semantic diagnostic catalog.
///
/// # Errors
///
/// Returns [`DiagnosticCatalogError`] if a built-in code or owner violates its
/// typed grammar, which indicates a programmer error in this crate.
pub fn r0_diagnostic_catalog()
-> Result<BTreeMap<DiagnosticCode, DiagnosticCatalogEntry>, DiagnosticCatalogError> {
    const COMPOSE_CODES: &[&str] = &[
        "compose.diagnostics_truncated",
        "compose.evaluation_failed",
        "compose.import_cycle",
        "compose.import_denied",
        "compose.import_depth",
        "compose.import_files",
        "compose.output_limit",
        "compose.path_collision",
        "compose.policy_violation",
        "compose.schema_mismatch",
        "compose.source_limit",
        "compose.worker_capability",
        "compose.worker_memory",
        "compose.worker_protocol",
        "compose.worker_recursion",
        "compose.worker_timeout",
        "compose.worker_unavailable",
    ];
    const SEMANTIC_CODES: &[&str] = &[
        "semantic.affordance_unavailable",
        "semantic.binding_rejected",
        "semantic.fallback_ambiguous",
        "semantic.fallback_cycle",
        "semantic.foreign_amendment_missing_dependency",
        "semantic.frozen_world_mismatch",
        "semantic.map_conflict",
        "semantic.map_merger_invalid",
        "semantic.predicate_type_mismatch",
        "semantic.role_ambiguous",
        "semantic.role_unsatisfied",
        "semantic.tag_cycle",
        "semantic.target_kind_mismatch",
        "semantic.unauthorized_contribution",
        "semantic.unknown_contract",
    ];
    let compose_owner = StableId::from_str("latticeaxiom:diagnostic-catalog/compose@1")?;
    let semantic_owner = StableId::from_str("latticeaxiom:diagnostic-catalog/semantic@1")?;
    COMPOSE_CODES
        .iter()
        .map(|code| (*code, &compose_owner))
        .chain(SEMANTIC_CODES.iter().map(|code| (*code, &semantic_owner)))
        .map(|(code_text, owner)| {
            let default_severity = if code_text == "compose.diagnostics_truncated" {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Error
            };
            let code = DiagnosticCode::from_str(code_text)?;
            Ok((
                code.clone(),
                DiagnosticCatalogEntry {
                    code,
                    owner: owner.clone(),
                    default_severity,
                },
            ))
        })
        .collect()
}

/// A diagnostic code violated the stable catalog grammar.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DiagnosticCodeError {
    /// The code did not contain canonical dot-separated segments.
    #[error("invalid diagnostic code `{value}`")]
    InvalidCode {
        /// Rejected code text.
        value: String,
    },
}

/// A built-in catalog entry could not be constructed.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum DiagnosticCatalogError {
    /// A catalog code violated the diagnostic grammar.
    #[error(transparent)]
    Code(#[from] DiagnosticCodeError),
    /// A catalog owner violated the stable-ID grammar.
    #[error(transparent)]
    Owner(#[from] IdentifierError),
}

/// An error produced while applying the deterministic diagnostic policy.
#[derive(Debug, Error)]
pub enum DiagnosticPolicyError {
    /// The policy reserved no slot for the required truncation diagnostic.
    #[error("retained diagnostic limit must be greater than zero")]
    ZeroLimit,
    /// A diagnostic could not be encoded for deterministic comparison.
    #[error("failed to encode a diagnostic for deterministic policy: {0}")]
    CanonicalEncoding(#[from] CanonicalJsonError),
}

#[cfg(test)]
mod tests {
    use latticeaxiom_core::{CanonicalHash, SourceSpan};

    use super::*;

    #[test]
    fn diagnostic_codes_validate_and_round_trip() {
        let code = DiagnosticCode::from_str("semantic.role_ambiguous")
            .unwrap_or_else(|error| panic!("valid diagnostic code was rejected: {error}"));
        let encoded = serde_json::to_string(&code).unwrap_or_default();
        assert_eq!(serde_json::from_str(&encoded).ok(), Some(code));
        for invalid in [
            "semantic",
            "Semantic.role",
            "semantic..role",
            "semantic.role!",
        ] {
            assert!(DiagnosticCode::from_str(invalid).is_err());
        }
    }

    #[test]
    fn r0_catalog_has_unique_typed_entries() {
        let catalog = r0_diagnostic_catalog()
            .unwrap_or_else(|error| panic!("built-in catalog is invalid: {error}"));
        assert_eq!(DIAGNOSTIC_CATALOG_VERSION, 2);
        assert_eq!(catalog.len(), 32);
        assert!(catalog.keys().all(|code| code.as_str().contains('.')));
        for code in [
            "compose.import_cycle",
            "compose.import_depth",
            "compose.import_files",
            "compose.worker_timeout",
            "compose.worker_memory",
            "compose.worker_recursion",
            "compose.worker_capability",
            "compose.worker_protocol",
            "compose.worker_unavailable",
            "compose.diagnostics_truncated",
        ] {
            let code = diagnostic_code(code);
            assert!(catalog.contains_key(&code), "catalog omitted {code}");
        }

        let truncation = catalog
            .get(&diagnostic_code("compose.diagnostics_truncated"))
            .unwrap_or_else(|| panic!("truncation diagnostic is absent"));
        assert_eq!(truncation.default_severity, DiagnosticSeverity::Warning);
        assert_eq!(
            truncation.owner.to_string(),
            "latticeaxiom:diagnostic-catalog/compose@1"
        );
    }

    #[test]
    fn diagnostics_sort_by_severity_code_and_primary_location() {
        let diagnostics = vec![
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Info,
                "info",
                Vec::new(),
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Warning,
                "warning",
                Vec::new(),
                Vec::new(),
            ),
            diagnostic(
                "compose.worker_timeout",
                DiagnosticSeverity::Error,
                "later-code",
                Vec::new(),
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "source-b",
                vec![label("latticeaxiom:source/b", "game.ncl", 1, 2, "label")],
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "path-b",
                vec![label(
                    "latticeaxiom:source/a",
                    "profiles/b.ncl",
                    1,
                    2,
                    "label",
                )],
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "span-two",
                vec![label(
                    "latticeaxiom:source/a",
                    "profiles/a.ncl",
                    2,
                    3,
                    "label",
                )],
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "span-one",
                vec![label(
                    "latticeaxiom:source/a",
                    "profiles/a.ncl",
                    1,
                    2,
                    "label",
                )],
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "no-source",
                Vec::new(),
                Vec::new(),
            ),
        ];

        let normalized = normalize_diagnostics(diagnostics, 32)
            .unwrap_or_else(|error| panic!("diagnostic normalization failed: {error}"));
        assert_eq!(
            summaries(&normalized),
            [
                "no-source",
                "span-one",
                "span-two",
                "path-b",
                "source-b",
                "later-code",
                "warning",
                "info",
            ]
        );
    }

    #[test]
    fn summary_canonical_labels_and_notes_complete_the_sort_key() {
        let primary = label("latticeaxiom:source/a", "profiles/a.ncl", 1, 2, "a-label");
        let alternate = label("latticeaxiom:source/a", "profiles/a.ncl", 1, 2, "b-label");
        let diagnostics = vec![
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "same",
                vec![alternate],
                vec!["a-note".to_owned()],
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "same",
                vec![primary.clone()],
                vec!["z-note".to_owned()],
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "same",
                vec![primary],
                vec!["a-note".to_owned()],
            ),
        ];

        let normalized = normalize_diagnostics(diagnostics, 32)
            .unwrap_or_else(|error| panic!("diagnostic normalization failed: {error}"));
        assert_eq!(normalized[0].labels[0].message, "a-label");
        assert_eq!(normalized[0].notes, ["a-note"]);
        assert_eq!(normalized[1].labels[0].message, "a-label");
        assert_eq!(normalized[1].notes, ["z-note"]);
        assert_eq!(normalized[2].labels[0].message, "b-label");
    }

    #[test]
    fn deduplication_requires_identical_complete_dto_bytes() {
        let base = diagnostic(
            "compose.import_cycle",
            DiagnosticSeverity::Error,
            "cycle",
            vec![label(
                "latticeaxiom:source/a",
                "game.ncl",
                1,
                2,
                "cycle edge",
            )],
            vec!["first note".to_owned()],
        );
        let mut distinct = base.clone();
        distinct.notes = vec!["second note".to_owned()];

        let normalized = normalize_diagnostics(vec![base.clone(), distinct, base], 32)
            .unwrap_or_else(|error| panic!("diagnostic normalization failed: {error}"));
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].notes, ["first note"]);
        assert_eq!(normalized[1].notes, ["second note"]);
    }

    #[test]
    fn truncation_reserves_the_last_slot_and_reports_counts() {
        let diagnostics = ["d", "b", "a", "c"]
            .into_iter()
            .map(|summary| {
                diagnostic(
                    "compose.import_cycle",
                    DiagnosticSeverity::Error,
                    summary,
                    Vec::new(),
                    Vec::new(),
                )
            })
            .collect();

        let normalized = normalize_diagnostics(diagnostics, 3)
            .unwrap_or_else(|error| panic!("diagnostic normalization failed: {error}"));
        assert_eq!(
            summaries(&normalized),
            ["a", "b", "diagnostic output was truncated"]
        );
        assert_eq!(normalized[2].code.as_str(), "compose.diagnostics_truncated");
        assert_eq!(normalized[2].severity, DiagnosticSeverity::Warning);
        assert_eq!(normalized[2].notes, ["total=4; retained=2; omitted=2"]);
    }

    #[test]
    fn one_slot_limit_returns_only_the_truncation_diagnostic() {
        let diagnostics = vec![
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "a",
                Vec::new(),
                Vec::new(),
            ),
            diagnostic(
                "compose.import_cycle",
                DiagnosticSeverity::Error,
                "b",
                Vec::new(),
                Vec::new(),
            ),
        ];

        let normalized = normalize_diagnostics(diagnostics, 1)
            .unwrap_or_else(|error| panic!("diagnostic normalization failed: {error}"));
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].code.as_str(), "compose.diagnostics_truncated");
        assert_eq!(normalized[0].notes, ["total=2; retained=0; omitted=2"]);
    }

    #[test]
    fn zero_limit_is_a_stable_domain_error() {
        assert!(matches!(
            normalize_diagnostics(Vec::new(), 0),
            Err(DiagnosticPolicyError::ZeroLimit)
        ));
    }

    fn diagnostic(
        code: &str,
        severity: DiagnosticSeverity,
        summary: &str,
        labels: Vec<DiagnosticLabel>,
        notes: Vec<String>,
    ) -> Diagnostic {
        Diagnostic {
            code: diagnostic_code(code),
            severity,
            summary: summary.to_owned(),
            labels,
            notes,
        }
    }

    fn diagnostic_code(value: &str) -> DiagnosticCode {
        DiagnosticCode::from_str(value)
            .unwrap_or_else(|error| panic!("fixture diagnostic code is invalid: {error}"))
    }

    fn label(
        source_id: &str,
        logical_path: &str,
        start_byte: u64,
        end_byte: u64,
        message: &str,
    ) -> DiagnosticLabel {
        let source_id = source_id
            .parse()
            .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}"));
        let span = SourceSpan::new(start_byte, end_byte)
            .unwrap_or_else(|error| panic!("fixture source span is invalid: {error}"));
        let provenance = SourceProvenance::new(
            source_id,
            logical_path,
            CanonicalHash::digest(logical_path.as_bytes()),
            Some(span),
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("fixture provenance is invalid: {error}"));
        DiagnosticLabel {
            provenance,
            message: message.to_owned(),
        }
    }

    fn summaries(diagnostics: &[Diagnostic]) -> Vec<&str> {
        diagnostics
            .iter()
            .map(|diagnostic| diagnostic.summary.as_str())
            .collect()
    }
}
