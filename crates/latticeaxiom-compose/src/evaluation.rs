//! Trusted ambient-filesystem Nickel evaluation into typed composition models.
//!
//! This module is the in-process typed conversion boundary. It deliberately
//! does not claim to enforce wall-clock, process-memory, or Nickel recursion
//! limits. It also retains Nickel's default filesystem import resolver and is
//! therefore only an adapter for import-free expressions or trusted local
//! authoring fixtures. The controlled-root evaluator requires a separate
//! source-table-backed loader and worker supervisor.

use std::{io, str::FromStr};

use latticeaxiom_core::canonical_json_bytes;
use malachite_base::num::conversion::{string::options::ToSciOptions, traits::ToSci};
use nickel_lang_core::{
    error::report::{ColorOpt, report_as_str},
    eval::cache::CacheImpl,
    eval::value::{Container, EnumVariantData, NickelValue, ValueContentRef},
    program::{Program, ProgramBuilder},
};
use serde::{Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::NickelEvaluationLimits;

/// A stable Lattice-owned failure from trusted Nickel typed evaluation.
///
/// Nickel and codespan error types do not cross this boundary. The logical
/// source name and rendered, color-free diagnostic are retained so a shared
/// diagnostic layer can attach its own severity and structured code.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum NickelEvaluationError {
    /// The in-memory Nickel program could not be constructed.
    #[error("[compose.evaluation_failed] `{source_name}`: {details}")]
    Input {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Stable textual failure detail.
        details: String,
    },
    /// Nickel parsing, import resolution, typechecking, or evaluation failed.
    #[error("[compose.evaluation_failed] `{source_name}`:\n{rendered}")]
    Evaluation {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Color-free Nickel diagnostic with source context.
        rendered: String,
    },
    /// The fully evaluated Nickel value could not be exported as exact JSON data.
    #[error("[compose.evaluation_failed] `{source_name}`: {details}")]
    Export {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Stable export failure detail.
        details: String,
    },
    /// The evaluated Nickel value did not match the requested Rust model.
    #[error("[compose.schema_mismatch] `{source_name}`: {details}")]
    Schema {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Serde schema mismatch detail.
        details: String,
    },
    /// The typed result could not be encoded as canonical Lattice data.
    #[error("[compose.evaluation_failed] `{source_name}`: {details}")]
    CanonicalEncoding {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Canonical encoder failure detail.
        details: String,
    },
    /// The canonical typed result exceeded the caller's explicit byte limit.
    #[error(
        "[compose.output_limit] `{source_name}`: canonical output is {actual_bytes} bytes, limit is {maximum_bytes} bytes"
    )]
    OutputLimitExceeded {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Explicit maximum accepted canonical output size.
        maximum_bytes: usize,
        /// Canonical byte size produced by the typed result.
        actual_bytes: usize,
    },
    /// The configured output limit cannot be represented on this platform.
    #[error(
        "[compose.output_limit] `{source_name}`: configured canonical output limit {configured_bytes} cannot be represented on this platform"
    )]
    OutputLimitUnsupported {
        /// Caller-provided resolver source name.
        source_name: String,
        /// Configured maximum expressed in the portable policy representation.
        configured_bytes: u64,
    },
}

impl NickelEvaluationError {
    /// Returns the stable diagnostic code for this failure class.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Input { .. }
            | Self::Evaluation { .. }
            | Self::Export { .. }
            | Self::CanonicalEncoding { .. } => "compose.evaluation_failed",
            Self::Schema { .. } => "compose.schema_mismatch",
            Self::OutputLimitExceeded { .. } | Self::OutputLimitUnsupported { .. } => {
                "compose.output_limit"
            }
        }
    }

    /// Returns the caller-provided resolver source name.
    #[must_use]
    pub fn source_name(&self) -> &str {
        match self {
            Self::Input { source_name, .. }
            | Self::Evaluation { source_name, .. }
            | Self::Export { source_name, .. }
            | Self::Schema { source_name, .. }
            | Self::CanonicalEncoding { source_name, .. }
            | Self::OutputLimitExceeded { source_name, .. }
            | Self::OutputLimitUnsupported { source_name, .. } => source_name,
        }
    }
}

/// Fully evaluates trusted Nickel source and directly deserializes its value.
///
/// Evaluation uses Nickel's export semantics and sends the `trace` builtin to
/// an I/O sink. The fully evaluated value is converted to an exact JSON value
/// and then decoded by the normative Rust serde model. This preserves both the
/// string-tagged enum encoding shared with the CLI golden corpus and finite
/// decimal precision instead of using Nickel's lossy `f64` Serde fallback.
/// The normalized typed canonical bytes are checked against
/// `limits.output_bytes`. Other fields in
/// [`NickelEvaluationLimits`] are recorded policy inputs for enforcement by the
/// import controller, worker supervisor, or an instrumented evaluator; this
/// function does not claim to enforce them.
///
/// `resolver_source_name` is passed directly to Nickel as the source path. For
/// import-free expressions it may be a stable logical diagnostic name. If the
/// expression imports relative paths, Nickel uses it as the filesystem base;
/// such diagnostics can therefore contain a machine-local physical path.
/// This function does not consume [`crate::CanonicalSourceTable`], does not
/// authorize imports, and does not prevent absolute or escaping imports. Never
/// pass untrusted source or treat this adapter as the R0 controlled-import
/// policy gate.
///
/// # Errors
///
/// Returns [`NickelEvaluationError`] when program construction, Nickel
/// evaluation, exact JSON-compatible export, typed deserialization, canonical
/// encoding, or the canonical output-size check fails. No partial typed result
/// is returned. A Nickel rational without a finite base-10 representation is
/// rejected because JSON and the R0 fixed-point model cannot represent it
/// exactly.
pub fn evaluate_trusted_nickel_source<T>(
    source: &str,
    resolver_source_name: impl Into<String>,
    limits: NickelEvaluationLimits,
) -> Result<T, NickelEvaluationError>
where
    T: DeserializeOwned + Serialize,
{
    let source_name = resolver_source_name.into();
    let maximum_bytes = usize::try_from(limits.output_bytes).map_err(|_| {
        NickelEvaluationError::OutputLimitUnsupported {
            source_name: source_name.clone(),
            configured_bytes: limits.output_bytes,
        }
    })?;
    let mut program: Program<CacheImpl> = ProgramBuilder::new()
        .add_source_string(source.to_owned(), source_name.clone())
        .with_trace(io::sink())
        .build()
        .map_err(|error| NickelEvaluationError::Input {
            source_name: source_name.clone(),
            details: error.to_string(),
        })?;

    let evaluated = program.eval_full_for_export().map_err(|error| {
        let mut files = program.files();
        NickelEvaluationError::Evaluation {
            source_name: source_name.clone(),
            rendered: report_as_str(&mut files, error, ColorOpt::Never),
        }
    })?;

    let exported =
        exact_json_value(&evaluated).map_err(|details| NickelEvaluationError::Export {
            source_name: source_name.clone(),
            details,
        })?;
    let typed =
        serde_json::from_value(exported).map_err(|error| NickelEvaluationError::Schema {
            source_name: source_name.clone(),
            details: error.to_string(),
        })?;

    let canonical =
        canonical_json_bytes(&typed).map_err(|error| NickelEvaluationError::CanonicalEncoding {
            source_name: source_name.clone(),
            details: error.to_string(),
        })?;
    let actual_bytes = canonical.len();

    if actual_bytes > maximum_bytes {
        return Err(NickelEvaluationError::OutputLimitExceeded {
            source_name,
            maximum_bytes,
            actual_bytes,
        });
    }

    Ok(typed)
}

fn exact_json_value(value: &NickelValue) -> Result<serde_json::Value, String> {
    match value.content_ref() {
        ValueContentRef::Null => Ok(serde_json::Value::Null),
        ValueContentRef::Bool(value) => Ok(serde_json::Value::Bool(value)),
        ValueContentRef::Number(number) => exact_json_number(number),
        ValueContentRef::String(value) => Ok(serde_json::Value::String(value.to_string())),
        ValueContentRef::EnumVariant(EnumVariantData { tag, arg: None }) => {
            Ok(serde_json::Value::String(tag.label().to_owned()))
        }
        ValueContentRef::EnumVariant(EnumVariantData { tag, arg: Some(_) }) => Err(format!(
            "cannot export enum variant `'{tag}` with a non-empty argument"
        )),
        ValueContentRef::Record(Container::Empty) => {
            Ok(serde_json::Value::Object(serde_json::Map::new()))
        }
        ValueContentRef::Record(Container::Alloc(record)) => {
            let mut entries = record
                .iter_serializable()
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| format!("record field `{}` has no definition", error.id))?;
            entries.sort_by_key(|(name, _)| *name);

            let mut object = serde_json::Map::new();
            for (name, value) in entries {
                object.insert(name.to_string(), exact_json_value(value)?);
            }
            Ok(serde_json::Value::Object(object))
        }
        ValueContentRef::Array(Container::Empty) => Ok(serde_json::Value::Array(Vec::new())),
        ValueContentRef::Array(Container::Alloc(array)) => array
            .array
            .iter()
            .map(exact_json_value)
            .collect::<Result<Vec<_>, _>>()
            .map(serde_json::Value::Array),
        _ => Err(format!(
            "cannot export a fully evaluated value of type {}",
            value.type_of().unwrap_or("unknown")
        )),
    }
}

fn exact_json_number(number: &nickel_lang_core::term::Number) -> Result<serde_json::Value, String> {
    if number.length_after_point_in_small_base(10).is_none() {
        return Err(format!(
            "Nickel number `{number}` has no finite base-10 representation"
        ));
    }

    let mut options = ToSciOptions::default();
    options.set_size_complete();
    let encoded = number.to_sci_with_options(options).to_string();
    let number = serde_json::Number::from_str(&encoded)
        .map_err(|error| format!("could not encode exact Nickel number `{encoded}`: {error}"))?;
    Ok(serde_json::Value::Number(number))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[derive(Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct FixtureSpec {
        name: String,
        count: u32,
    }

    #[derive(Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PreciseNumberSpec {
        value: serde_json::Number,
    }

    #[derive(Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct NormalizedSetSpec {
        values: BTreeSet<String>,
    }

    #[test]
    fn trusted_expression_deserializes_directly_to_a_typed_model() {
        let spec = evaluate_trusted_nickel_source::<FixtureSpec>(
            r#"{ name = "Terrenia", count = 3 }"#,
            "fixtures/profile.ncl",
            NickelEvaluationLimits::default(),
        )
        .unwrap_or_else(|error| panic!("trusted fixture did not evaluate: {error}"));

        assert_eq!(
            spec,
            FixtureSpec {
                name: "Terrenia".to_owned(),
                count: 3,
            }
        );
    }

    #[test]
    fn schema_mismatch_has_a_stable_lattice_error_code_and_source() {
        let result = evaluate_trusted_nickel_source::<FixtureSpec>(
            r#"{ name = "Terrenia", count = "many" }"#,
            "fixtures/schema-error.ncl",
            NickelEvaluationLimits::default(),
        );
        let Err(error) = result else {
            panic!("a string count must not deserialize as an integer");
        };

        assert_eq!(error.code(), "compose.schema_mismatch");
        assert_eq!(error.source_name(), "fixtures/schema-error.ncl");
        assert!(matches!(error, NickelEvaluationError::Schema { .. }));
    }

    #[test]
    fn eighteen_digit_decimal_crosses_the_boundary_without_rounding() {
        let spec = evaluate_trusted_nickel_source::<PreciseNumberSpec>(
            r"{ value = 0.123456789012345678 }",
            "fixtures/precise-number.ncl",
            NickelEvaluationLimits::default(),
        )
        .unwrap_or_else(|error| panic!("precise number fixture did not evaluate: {error}"));

        assert_eq!(spec.value.to_string(), "0.123456789012345678");
    }

    #[test]
    fn non_terminating_rational_is_rejected_instead_of_rounded() {
        let result = evaluate_trusted_nickel_source::<PreciseNumberSpec>(
            r"{ value = 1 / 3 }",
            "fixtures/non-terminating-number.ncl",
            NickelEvaluationLimits::default(),
        );
        let Err(error) = result else {
            panic!("a non-terminating rational must not be rounded into JSON");
        };

        assert_eq!(error.code(), "compose.evaluation_failed");
        assert!(matches!(error, NickelEvaluationError::Export { .. }));
    }

    #[test]
    fn canonical_limit_is_measured_after_typed_normalization() {
        let expected = NormalizedSetSpec {
            values: BTreeSet::from(["Terrenia".to_owned()]),
        };
        let canonical = canonical_json_bytes(&expected)
            .unwrap_or_else(|error| panic!("normalized fixture did not canonicalize: {error}"));
        let limits = NickelEvaluationLimits {
            output_bytes: u64::try_from(canonical.len()).unwrap_or_else(|error| {
                panic!("canonical fixture size must fit the portable policy field: {error}")
            }),
            ..NickelEvaluationLimits::default()
        };

        let actual = evaluate_trusted_nickel_source::<NormalizedSetSpec>(
            r#"{ values = ["Terrenia", "Terrenia", "Terrenia"] }"#,
            "fixtures/normalized-set.ncl",
            limits,
        )
        .unwrap_or_else(|error| panic!("normalized set fixture did not evaluate: {error}"));

        assert_eq!(actual, expected);
    }

    #[test]
    fn canonical_output_larger_than_the_explicit_limit_is_rejected() {
        let limits = NickelEvaluationLimits {
            output_bytes: 1,
            ..NickelEvaluationLimits::default()
        };
        let result = evaluate_trusted_nickel_source::<FixtureSpec>(
            r#"{ name = "Terrenia", count = 3 }"#,
            "fixtures/output-limit.ncl",
            limits,
        );
        let Err(error) = result else {
            panic!("the typed fixture must exceed a one-byte canonical limit");
        };

        match error {
            NickelEvaluationError::OutputLimitExceeded {
                maximum_bytes,
                actual_bytes,
                ..
            } => {
                assert_eq!(maximum_bytes, 1);
                assert!(actual_bytes > maximum_bytes);
            }
            other => panic!("expected output limit failure, got {other}"),
        }
    }
}
