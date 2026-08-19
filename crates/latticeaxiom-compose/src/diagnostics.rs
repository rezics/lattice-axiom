//! Stable diagnostics that cross composition and persistence boundaries.

use std::{collections::BTreeMap, fmt, str::FromStr};

use latticeaxiom_core::{IdentifierError, SourceProvenance, StableId};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use thiserror::Error;

/// Current version of the composition diagnostic-code catalog.
pub const DIAGNOSTIC_CATALOG_VERSION: u32 = 1;

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

/// Returns the minimum R0 composition and semantic diagnostic catalog.
///
/// # Errors
///
/// Returns [`DiagnosticCatalogError`] if a built-in code or owner violates its
/// typed grammar, which indicates a programmer error in this crate.
pub fn r0_diagnostic_catalog()
-> Result<BTreeMap<DiagnosticCode, DiagnosticCatalogEntry>, DiagnosticCatalogError> {
    const COMPOSE_CODES: &[&str] = &[
        "compose.evaluation_failed",
        "compose.import_denied",
        "compose.output_limit",
        "compose.path_collision",
        "compose.policy_violation",
        "compose.schema_mismatch",
        "compose.source_limit",
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
        .map(|(code, owner)| {
            let code = DiagnosticCode::from_str(code)?;
            Ok((
                code.clone(),
                DiagnosticCatalogEntry {
                    code,
                    owner: owner.clone(),
                    default_severity: DiagnosticSeverity::Error,
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

#[cfg(test)]
mod tests {
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
        assert_eq!(catalog.len(), 22);
        assert!(catalog.keys().all(|code| code.as_str().contains('.')));
    }
}
