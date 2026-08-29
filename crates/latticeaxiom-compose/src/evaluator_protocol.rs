//! Versioned framed protocol between the composition controller and evaluator worker.
//!
//! The protocol carries only Lattice-owned stable DTOs and immutable source
//! snapshots. It records expected and reported enforcement facts, but it does
//! not install, discover, or prove worker containment capabilities.

use std::collections::BTreeMap;

use latticeaxiom_core::{CanonicalHash, SourceProvenance, TargetTriple, canonical_json_bytes};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use thiserror::Error;

use crate::{
    COMPOSITION_SCHEMA_VERSION, CompositionSpec, DIAGNOSTIC_CATALOG_VERSION, Diagnostic,
    DiagnosticSeverity, EvaluationPolicyReceipt, GAME_PROFILE_MODEL_VERSION,
    NICKEL_LIBRARY_CONTRACT_MAJOR, PACKAGE_MODEL_VERSION, PackageSpec, R0_AUTHORING_CORPUS_MAJOR,
    SourceClosureReceipt, SourceClosureRequest, SourceSnapshot, normalize_diagnostics,
};

/// Current evaluator worker protocol version.
pub const WORKER_PROTOCOL_VERSION: u32 = 1;

/// Maximum JSON payload bytes carried by one worker protocol frame.
///
/// The four-byte frame prefix is not included in this limit. The controller
/// must still apply the complete-composition deadline and containment memory
/// policy while reading a frame.
pub const WORKER_PROTOCOL_MAX_PAYLOAD_BYTES: u32 = 256 * 1024 * 1024;

/// Number of big-endian bytes in the worker frame length prefix.
pub const WORKER_PROTOCOL_LENGTH_PREFIX_BYTES: usize = 4;

/// Absolute protocol cap used before request-specific diagnostic limits are known.
pub const WORKER_PROTOCOL_MAX_DIAGNOSTICS: usize = 4_096;

/// All exact version axes carried by one request and response.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerProtocolVersions {
    /// Framed evaluator protocol version.
    pub protocol: u32,
    /// Stable diagnostic catalog version.
    pub diagnostic_catalog: u32,
    /// Normalized composition schema version.
    pub composition_schema: u32,
    /// Package authoring model version.
    pub package_model: u32,
    /// Game-profile authoring model version.
    pub game_profile_model: u32,
    /// `latticeaxiom.lib` contract major.
    pub library_contract_major: u32,
    /// Executable authoring corpus major.
    pub corpus_major: u32,
}

impl WorkerProtocolVersions {
    /// Returns the exact versions implemented by this build.
    #[must_use]
    pub const fn current() -> Self {
        Self {
            protocol: WORKER_PROTOCOL_VERSION,
            diagnostic_catalog: DIAGNOSTIC_CATALOG_VERSION,
            composition_schema: COMPOSITION_SCHEMA_VERSION,
            package_model: PACKAGE_MODEL_VERSION,
            game_profile_model: GAME_PROFILE_MODEL_VERSION,
            library_contract_major: NICKEL_LIBRARY_CONTRACT_MAJOR,
            corpus_major: R0_AUTHORING_CORPUS_MAJOR,
        }
    }

    fn validate(self) -> Result<(), WorkerProtocolError> {
        validate_version("protocol", self.protocol, WORKER_PROTOCOL_VERSION)?;
        validate_version(
            "diagnostic_catalog",
            self.diagnostic_catalog,
            DIAGNOSTIC_CATALOG_VERSION,
        )?;
        validate_version(
            "composition_schema",
            self.composition_schema,
            COMPOSITION_SCHEMA_VERSION,
        )?;
        validate_version("package_model", self.package_model, PACKAGE_MODEL_VERSION)?;
        validate_version(
            "game_profile_model",
            self.game_profile_model,
            GAME_PROFILE_MODEL_VERSION,
        )?;
        validate_version(
            "library_contract_major",
            self.library_contract_major,
            NICKEL_LIBRARY_CONTRACT_MAJOR,
        )?;
        validate_version("corpus_major", self.corpus_major, R0_AUTHORING_CORPUS_MAJOR)
    }
}

/// Trusted worker invocation class selected by the controller.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WorkerExecutionClass {
    /// Developer tooling controlled by the local operator.
    TrustedTool,
    /// Repository-owned deterministic fixture evaluation.
    TrustedFixture,
}

/// Normative authoring model selected for the entry source.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case", tag = "kind")]
pub enum AuthoringTarget {
    /// Evaluate a package authoring entry as part of composition.
    Package,
    /// Evaluate a game profile and normalize it for a concrete target.
    GameProfile {
        /// Target used by profile package selection and normalized policy.
        composition_target: TargetTriple,
        /// Kernel-owned provenance attached to the normalized composition.
        provenance: Box<SourceProvenance>,
    },
}

/// One complete immutable evaluation request sent to a worker.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRequest {
    /// Exact protocol, catalog, model, library, and corpus axes.
    pub versions: WorkerProtocolVersions,
    /// Normative model selected for the entry source.
    pub authoring_target: AuthoringTarget,
    /// Containment host target on which the worker executes.
    pub worker_target: TargetTriple,
    /// Trusted invocation class authorized by the controller.
    pub execution_class: WorkerExecutionClass,
    /// Entry, root grants, aliases, and exact evaluation limits.
    pub source_closure: SourceClosureRequest,
    /// Complete immutable snapshots in strict canonical source-ID order.
    pub snapshots: Vec<SourceSnapshot>,
    /// Controller-expected policy and enforcement receipt.
    ///
    /// The controller is responsible for constructing this only after it has
    /// installed the named containment backends. Structural protocol
    /// validation cannot prove that installation occurred.
    pub expected_policy: EvaluationPolicyReceipt,
}

impl WorkerRequest {
    /// Validates protocol versions, grants, snapshots, and policy binding.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] when a version axis is unsupported,
    /// grant order or authority is invalid, snapshots are not exact and
    /// self-consistent, or the expected policy receipt disagrees with the
    /// request target and limits.
    pub fn validate(&self) -> Result<(), WorkerProtocolError> {
        self.versions.validate()?;
        if self.versions.library_contract_major != self.source_closure.library_contract_major
            || self.versions.corpus_major != self.source_closure.corpus_major
        {
            return Err(WorkerProtocolError::InvalidRequest {
                details: "source closure version axes differ from protocol versions".to_owned(),
            });
        }
        self.expected_policy
            .validate()
            .map_err(|error| WorkerProtocolError::InvalidRequest {
                details: format!("invalid expected policy receipt: {error}"),
            })?;
        if self.worker_target != self.expected_policy.target {
            return Err(WorkerProtocolError::InvalidRequest {
                details: "worker target differs from expected policy receipt".to_owned(),
            });
        }
        if self.source_closure.limits != self.expected_policy.limits {
            return Err(WorkerProtocolError::InvalidRequest {
                details: "source closure limits differ from expected policy receipt".to_owned(),
            });
        }
        self.source_closure
            .validate()
            .map_err(|error| WorkerProtocolError::InvalidRequest {
                details: format!("invalid source closure request: {error}"),
            })?;
        if !self
            .source_closure
            .root_grants
            .windows(2)
            .all(|pair| pair[0].source_id < pair[1].source_id)
        {
            return Err(WorkerProtocolError::InvalidRequest {
                details: "root grants must be strictly ordered by source ID".to_owned(),
            });
        }
        if self.snapshots.len() != self.source_closure.root_grants.len() {
            return Err(WorkerProtocolError::InvalidRequest {
                details: "snapshot set must exactly match the granted root set".to_owned(),
            });
        }

        if !self
            .snapshots
            .windows(2)
            .all(|pair| pair[0].source_id() < pair[1].source_id())
        {
            return Err(WorkerProtocolError::InvalidRequest {
                details: "snapshots must be strictly ordered by source ID".to_owned(),
            });
        }
        let grants = self
            .source_closure
            .root_grants
            .iter()
            .map(|grant| (&grant.source_id, grant))
            .collect::<BTreeMap<_, _>>();
        for snapshot in &self.snapshots {
            let source_id = snapshot.source_id();
            snapshot
                .verify()
                .map_err(|error| WorkerProtocolError::InvalidRequest {
                    details: format!("invalid snapshot `{source_id}`: {error}"),
                })?;
            let grant = grants.get(source_id).copied().ok_or_else(|| {
                WorkerProtocolError::InvalidRequest {
                    details: format!("snapshot `{source_id}` has no matching root grant"),
                }
            })?;
            if snapshot.root_kind() != grant.root_kind {
                return Err(WorkerProtocolError::InvalidRequest {
                    details: format!("snapshot `{source_id}` root kind differs from its grant"),
                });
            }
            if snapshot.source_hash() != grant.source_hash {
                return Err(WorkerProtocolError::InvalidRequest {
                    details: format!("snapshot `{source_id}` source hash differs from its grant"),
                });
            }
        }
        if let AuthoringTarget::GameProfile { provenance, .. } = &self.authoring_target {
            validate_entry_provenance(provenance, self)
                .map_err(|details| WorkerProtocolError::InvalidRequest { details })?;
        }
        Ok(())
    }
}

/// Canonical normalized `CompositionSpec` bytes and their exact byte digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalCompositionSpec {
    /// Canonical JSON bytes of a validated normalized composition.
    pub canonical_bytes: Vec<u8>,
    /// SHA-256 digest of `canonical_bytes`.
    pub canonical_hash: CanonicalHash,
}

impl CanonicalCompositionSpec {
    /// Creates canonical bytes and an exact byte hash from a normalized spec.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] when the typed composition is invalid
    /// or canonical JSON encoding fails.
    pub fn from_spec(spec: &CompositionSpec) -> Result<Self, WorkerProtocolError> {
        spec.validate()
            .map_err(|error| WorkerProtocolError::InvalidComposition {
                details: error.to_string(),
            })?;
        let canonical_bytes = canonical_json_bytes(spec).map_err(|error| {
            WorkerProtocolError::InvalidComposition {
                details: format!("canonical encoding failed: {error}"),
            }
        })?;
        let canonical_hash = CanonicalHash::digest(&canonical_bytes);
        Ok(Self {
            canonical_bytes,
            canonical_hash,
        })
    }

    /// Decodes and revalidates the normalized composition and byte identity.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] when bytes exceed `maximum_bytes`, do
    /// not decode to the normative type, are not its canonical encoding, or
    /// disagree with the recorded hash.
    pub fn decode(&self, maximum_bytes: u64) -> Result<CompositionSpec, WorkerProtocolError> {
        let actual_bytes = u64::try_from(self.canonical_bytes.len()).map_err(|_| {
            WorkerProtocolError::InvalidComposition {
                details: "canonical output length exceeds portable representation".to_owned(),
            }
        })?;
        if actual_bytes > maximum_bytes {
            return Err(WorkerProtocolError::InvalidComposition {
                details: format!(
                    "canonical output bytes {actual_bytes} exceed limit {maximum_bytes}"
                ),
            });
        }
        let spec =
            serde_json::from_slice::<CompositionSpec>(&self.canonical_bytes).map_err(|error| {
                WorkerProtocolError::InvalidComposition {
                    details: format!("typed composition decoding failed: {error}"),
                }
            })?;
        spec.validate()
            .map_err(|error| WorkerProtocolError::InvalidComposition {
                details: error.to_string(),
            })?;
        let canonical = canonical_json_bytes(&spec).map_err(|error| {
            WorkerProtocolError::InvalidComposition {
                details: format!("canonical re-encoding failed: {error}"),
            }
        })?;
        if canonical != self.canonical_bytes {
            return Err(WorkerProtocolError::InvalidComposition {
                details: "composition bytes are not the canonical typed encoding".to_owned(),
            });
        }
        if CanonicalHash::digest(&self.canonical_bytes) != self.canonical_hash {
            return Err(WorkerProtocolError::InvalidComposition {
                details: "composition byte hash mismatch".to_owned(),
            });
        }
        Ok(spec)
    }
}

/// Canonical validated `PackageSpec` bytes and their exact byte digest.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CanonicalPackageSpec {
    /// Canonical JSON bytes of a validated package.
    pub canonical_bytes: Vec<u8>,
    /// SHA-256 digest of `canonical_bytes`.
    pub canonical_hash: CanonicalHash,
}

impl CanonicalPackageSpec {
    /// Creates canonical bytes and their exact hash from a package spec.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] if the package is invalid or canonical
    /// JSON encoding fails.
    pub fn from_spec(spec: &PackageSpec) -> Result<Self, WorkerProtocolError> {
        spec.validate()
            .map_err(|error| WorkerProtocolError::InvalidPackage {
                details: error.to_string(),
            })?;
        let canonical_bytes =
            canonical_json_bytes(spec).map_err(|error| WorkerProtocolError::InvalidPackage {
                details: format!("canonical encoding failed: {error}"),
            })?;
        Ok(Self {
            canonical_hash: CanonicalHash::digest(&canonical_bytes),
            canonical_bytes,
        })
    }

    /// Decodes and revalidates the package and its byte identity.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] when the output is oversized, invalid,
    /// non-canonical, or disagrees with its recorded hash.
    pub fn decode(&self, maximum_bytes: u64) -> Result<PackageSpec, WorkerProtocolError> {
        let actual_bytes = u64::try_from(self.canonical_bytes.len()).map_err(|_| {
            WorkerProtocolError::InvalidPackage {
                details: "canonical output length exceeds portable representation".to_owned(),
            }
        })?;
        if actual_bytes > maximum_bytes {
            return Err(WorkerProtocolError::InvalidPackage {
                details: format!(
                    "canonical output bytes {actual_bytes} exceed limit {maximum_bytes}"
                ),
            });
        }
        let spec =
            serde_json::from_slice::<PackageSpec>(&self.canonical_bytes).map_err(|error| {
                WorkerProtocolError::InvalidPackage {
                    details: format!("typed package decoding failed: {error}"),
                }
            })?;
        spec.validate()
            .map_err(|error| WorkerProtocolError::InvalidPackage {
                details: error.to_string(),
            })?;
        let canonical =
            canonical_json_bytes(&spec).map_err(|error| WorkerProtocolError::InvalidPackage {
                details: format!("canonical re-encoding failed: {error}"),
            })?;
        if canonical != self.canonical_bytes {
            return Err(WorkerProtocolError::InvalidPackage {
                details: "package bytes are not the canonical typed encoding".to_owned(),
            });
        }
        if CanonicalHash::digest(&self.canonical_bytes) != self.canonical_hash {
            return Err(WorkerProtocolError::InvalidPackage {
                details: "package byte hash mismatch".to_owned(),
            });
        }
        Ok(spec)
    }
}

/// Exact typed output produced by a successful worker evaluation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "kind",
    content = "output"
)]
pub enum WorkerOutput {
    /// Validated authored package output.
    Package(CanonicalPackageSpec),
    /// Validated normalized game composition output.
    Composition(CanonicalCompositionSpec),
}

/// Successful atomic output from one complete composition evaluation.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerSuccess {
    /// Canonical validated output matching the request authoring target.
    pub output: WorkerOutput,
    /// Complete static source-closure receipt.
    pub source_closure: SourceClosureReceipt,
    /// Canonically ordered, deduplicated, and bounded diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Worker-reported policy and enforcement receipt.
    pub evaluation: EvaluationPolicyReceipt,
}

/// Failed atomic output with no partial composition or source closure.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerFailure {
    /// Canonically ordered, deduplicated, and bounded failure diagnostics.
    pub diagnostics: Vec<Diagnostic>,
    /// Source-closure receipt when closure construction completed truthfully.
    pub source_closure: Option<SourceClosureReceipt>,
    /// Worker-reported receipt when policy installation completed truthfully.
    pub evaluation: Option<EvaluationPolicyReceipt>,
}

/// Exactly one atomic worker outcome.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    deny_unknown_fields,
    rename_all = "kebab-case",
    tag = "status",
    content = "payload"
)]
pub enum WorkerResponse {
    /// Complete success with canonical output and receipts.
    Success {
        /// Exact version axes used to produce the response.
        versions: WorkerProtocolVersions,
        /// Atomic successful result.
        result: WorkerSuccess,
    },
    /// Complete failure with no partial normalized output.
    Failure {
        /// Exact version axes used to produce the response.
        versions: WorkerProtocolVersions,
        /// Atomic failure result.
        failure: WorkerFailure,
    },
}

impl WorkerResponse {
    /// Validates versioning, diagnostic bounds, output identity, and receipts.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] for unsupported versions, malformed
    /// receipts, non-canonical or oversized diagnostics/output, success with
    /// error diagnostics, or failure without an error diagnostic or the
    /// canonical one-slot truncation summary.
    pub fn validate(&self) -> Result<(), WorkerProtocolError> {
        self.versions().validate()?;
        match self {
            Self::Success { result, .. } => {
                result.evaluation.validate().map_err(|error| {
                    WorkerProtocolError::InvalidResponse {
                        details: format!("invalid evaluation receipt: {error}"),
                    }
                })?;
                validate_diagnostics(&result.diagnostics, &result.evaluation)?;
                if result
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
                {
                    return Err(WorkerProtocolError::InvalidResponse {
                        details: "successful response contains an error diagnostic".to_owned(),
                    });
                }
                match &result.output {
                    WorkerOutput::Package(output) => {
                        let _package = output.decode(result.evaluation.limits.output_bytes)?;
                    }
                    WorkerOutput::Composition(output) => {
                        let composition = output.decode(result.evaluation.limits.output_bytes)?;
                        if composition.policy.evaluation_policy != result.evaluation.policy
                            || composition.policy.evaluation_limits != result.evaluation.limits
                        {
                            return Err(WorkerProtocolError::InvalidResponse {
                                details: "composition policy differs from evaluation receipt"
                                    .to_owned(),
                            });
                        }
                    }
                }
                result.source_closure.verify().map_err(|error| {
                    WorkerProtocolError::InvalidResponse {
                        details: format!("invalid source closure receipt: {error}"),
                    }
                })?;
                if result.source_closure.imported_files > result.evaluation.limits.imported_files
                    || result.source_closure.source_bytes > result.evaluation.limits.source_bytes
                    || result.source_closure.maximum_depth > result.evaluation.limits.import_depth
                {
                    return Err(WorkerProtocolError::InvalidResponse {
                        details: "source closure meters exceed evaluation limits".to_owned(),
                    });
                }
                Ok(())
            }
            Self::Failure { failure, .. } => {
                if let Some(evaluation) = &failure.evaluation {
                    evaluation.validate().map_err(|error| {
                        WorkerProtocolError::InvalidResponse {
                            details: format!("invalid failure evaluation receipt: {error}"),
                        }
                    })?;
                    validate_diagnostics(&failure.diagnostics, evaluation)?;
                } else {
                    validate_diagnostics_with_limit(
                        &failure.diagnostics,
                        WORKER_PROTOCOL_MAX_DIAGNOSTICS,
                    )?;
                }
                if let Some(source_closure) = &failure.source_closure {
                    source_closure.verify().map_err(|error| {
                        WorkerProtocolError::InvalidResponse {
                            details: format!("invalid failure source closure receipt: {error}"),
                        }
                    })?;
                    if let Some(evaluation) = &failure.evaluation
                        && (source_closure.imported_files > evaluation.limits.imported_files
                            || source_closure.source_bytes > evaluation.limits.source_bytes
                            || source_closure.maximum_depth > evaluation.limits.import_depth)
                    {
                        return Err(WorkerProtocolError::InvalidResponse {
                            details: "failure source closure meters exceed evaluation limits"
                                .to_owned(),
                        });
                    }
                }
                if !failure
                    .diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
                    && !is_one_slot_truncation_failure(failure)
                {
                    return Err(WorkerProtocolError::InvalidResponse {
                        details: "failed response requires an error diagnostic or the canonical one-slot truncation summary".to_owned(),
                    });
                }
                Ok(())
            }
        }
    }

    /// Validates a worker result against the exact controller request.
    ///
    /// This compares the reported policy receipt with the controller's
    /// expected receipt and binds success to the requested entry, snapshots,
    /// grants, and source bytes. Proving that the controller installed the
    /// named hard capabilities remains the controller's responsibility.
    ///
    /// # Errors
    ///
    /// Returns [`WorkerProtocolError`] when either message is invalid or the
    /// response substitutes policy, source authority, entry, or source bytes.
    #[cfg(feature = "nickel-evaluator")]
    pub fn validate_against_request(
        &self,
        request: &WorkerRequest,
    ) -> Result<(), WorkerProtocolError> {
        request.validate()?;
        self.validate()?;
        if self.versions() != request.versions {
            return Err(WorkerProtocolError::InvalidResponse {
                details: "response version axes differ from request".to_owned(),
            });
        }
        match self {
            Self::Success { result, .. } => {
                result
                    .evaluation
                    .validate_against_expected(&request.expected_policy)
                    .map_err(|error| WorkerProtocolError::InvalidResponse {
                        details: format!(
                            "evaluation receipt differs from controller request: {error}"
                        ),
                    })?;
                validate_diagnostics(&result.diagnostics, &request.expected_policy)?;
                validate_output_against_request(&result.output, request)?;
                validate_closure_against_request(&result.source_closure, request)?;
            }
            Self::Failure { failure, .. } => {
                validate_diagnostics(&failure.diagnostics, &request.expected_policy)?;
                if let Some(evaluation) = &failure.evaluation {
                    evaluation
                        .validate_against_expected(&request.expected_policy)
                        .map_err(|error| WorkerProtocolError::InvalidResponse {
                            details: format!(
                                "failure evaluation receipt differs from controller request: {error}"
                            ),
                        })?;
                }
                if let Some(source_closure) = &failure.source_closure {
                    validate_closure_against_request(source_closure, request)?;
                }
            }
        }
        Ok(())
    }

    /// Returns all exact version axes carried by either outcome.
    #[must_use]
    pub const fn versions(&self) -> WorkerProtocolVersions {
        match self {
            Self::Success { versions, .. } | Self::Failure { versions, .. } => *versions,
        }
    }
}

fn is_one_slot_truncation_failure(failure: &WorkerFailure) -> bool {
    let Some(evaluation) = &failure.evaluation else {
        return false;
    };
    if evaluation.limits.retained_diagnostics != 1 {
        return false;
    }
    let [diagnostic] = failure.diagnostics.as_slice() else {
        return false;
    };
    if diagnostic.code.as_str() != "compose.diagnostics_truncated"
        || diagnostic.severity != DiagnosticSeverity::Warning
        || diagnostic.summary != "diagnostic output was truncated"
        || !diagnostic.labels.is_empty()
    {
        return false;
    }
    let [note] = diagnostic.notes.as_slice() else {
        return false;
    };
    let mut counts = note.split("; ");
    let total = counts
        .next()
        .and_then(|value| value.strip_prefix("total="))
        .and_then(|value| value.parse::<usize>().ok());
    let retained = counts.next();
    let omitted = counts
        .next()
        .and_then(|value| value.strip_prefix("omitted="))
        .and_then(|value| value.parse::<usize>().ok());
    matches!(
        (total, retained, omitted, counts.next()),
        (Some(total), Some("retained=0"), Some(omitted), None)
            if total > 1 && omitted == total
    )
}

#[cfg(feature = "nickel-evaluator")]
fn validate_output_against_request(
    output: &WorkerOutput,
    request: &WorkerRequest,
) -> Result<(), WorkerProtocolError> {
    match (&request.authoring_target, output) {
        (AuthoringTarget::Package, WorkerOutput::Package(package)) => {
            let package = package.decode(request.expected_policy.limits.output_bytes)?;
            validate_entry_provenance(&package.provenance, request)
                .map_err(|details| WorkerProtocolError::InvalidResponse { details })?;
            Ok(())
        }
        (
            AuthoringTarget::GameProfile {
                composition_target,
                provenance,
            },
            WorkerOutput::Composition(composition),
        ) => {
            let composition = composition.decode(request.expected_policy.limits.output_bytes)?;
            if &composition.policy.target != composition_target
                || &composition.provenance != provenance.as_ref()
            {
                return Err(WorkerProtocolError::InvalidResponse {
                    details: "composition target or provenance differs from request".to_owned(),
                });
            }
            Ok(())
        }
        _ => Err(WorkerProtocolError::InvalidResponse {
            details: "success output kind differs from authoring target".to_owned(),
        }),
    }
}

#[cfg(feature = "nickel-evaluator")]
fn validate_closure_against_request(
    closure: &SourceClosureReceipt,
    request: &WorkerRequest,
) -> Result<(), WorkerProtocolError> {
    if closure.entry != request.source_closure.entry {
        return Err(WorkerProtocolError::InvalidResponse {
            details: "source closure entry differs from request".to_owned(),
        });
    }
    if closure.imported_files > request.source_closure.limits.imported_files
        || closure.source_bytes > request.source_closure.limits.source_bytes
        || closure.maximum_depth > request.source_closure.limits.import_depth
    {
        return Err(WorkerProtocolError::InvalidResponse {
            details: "source closure meters exceed request limits".to_owned(),
        });
    }
    let snapshots = request
        .snapshots
        .iter()
        .cloned()
        .map(|snapshot| (snapshot.source_id().clone(), snapshot))
        .collect::<BTreeMap<_, _>>();
    closure
        .verify_against_request_and_snapshot(&request.source_closure, &snapshots)
        .map_err(|error| WorkerProtocolError::InvalidResponse {
            details: format!("source closure differs from authored immutable closure: {error}"),
        })?;
    Ok(())
}

fn validate_entry_provenance(
    provenance: &SourceProvenance,
    request: &WorkerRequest,
) -> Result<(), String> {
    let entry = &request.source_closure.entry;
    let snapshot = request
        .snapshots
        .iter()
        .find(|snapshot| snapshot.source_id() == entry.source_id())
        .ok_or_else(|| format!("entry snapshot `{}` is absent", entry.source_id()))?;
    let file = snapshot
        .files()
        .get(entry.logical_path())
        .ok_or_else(|| format!("entry source `{entry}` is absent from its snapshot"))?;
    if provenance.source_id() != entry.source_id()
        || provenance.logical_path() != entry.logical_path()
        || provenance.content_hash() != file.receipt().content_hash()
    {
        return Err(format!(
            "entry provenance does not match immutable source `{entry}`"
        ));
    }
    Ok(())
}

/// Encodes one validated request as a four-byte-length-prefixed JSON frame.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] when request validation or serialization
/// fails, or when the encoded payload exceeds
/// [`WORKER_PROTOCOL_MAX_PAYLOAD_BYTES`].
pub fn encode_worker_request(request: &WorkerRequest) -> Result<Vec<u8>, WorkerProtocolError> {
    request.validate()?;
    encode_frame(request)
}

/// Decodes and validates exactly one request frame.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] for a short header, oversized or truncated
/// payload, trailing bytes, unknown or malformed JSON fields, unsupported
/// versions, or invalid request authority.
pub fn decode_worker_request(frame: &[u8]) -> Result<WorkerRequest, WorkerProtocolError> {
    let request: WorkerRequest = decode_frame(frame)?;
    request.validate()?;
    Ok(request)
}

/// Encodes one validated response as a four-byte-length-prefixed JSON frame.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] when response validation or serialization
/// fails, or when the encoded payload exceeds
/// [`WORKER_PROTOCOL_MAX_PAYLOAD_BYTES`].
pub fn encode_worker_response(response: &WorkerResponse) -> Result<Vec<u8>, WorkerProtocolError> {
    response.validate()?;
    encode_frame(response)
}

/// Decodes and validates exactly one response frame.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] for a short header, oversized or truncated
/// payload, trailing bytes, unknown or malformed JSON fields, unsupported
/// versions, or an invalid atomic result.
pub fn decode_worker_response(frame: &[u8]) -> Result<WorkerResponse, WorkerProtocolError> {
    let response: WorkerResponse = decode_frame(frame)?;
    response.validate()?;
    Ok(response)
}

/// Encodes one validated request payload without a frame prefix.
///
/// This adapter is used by [`crate::WorkerSupervisor`], which owns process
/// framing and deadline enforcement.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] when request validation or JSON encoding
/// fails, or when the payload exceeds the protocol cap.
pub fn encode_worker_request_payload(
    request: &WorkerRequest,
) -> Result<Vec<u8>, WorkerProtocolError> {
    request.validate()?;
    encode_payload(request)
}

/// Decodes one unframed worker request payload.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] for malformed JSON or an invalid request.
pub fn decode_worker_request_payload(payload: &[u8]) -> Result<WorkerRequest, WorkerProtocolError> {
    let request: WorkerRequest = decode_payload(payload)?;
    request.validate()?;
    Ok(request)
}

/// Encodes one validated response payload without a frame prefix.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] when response validation or JSON encoding
/// fails, or when the payload exceeds the protocol cap.
pub fn encode_worker_response_payload(
    response: &WorkerResponse,
) -> Result<Vec<u8>, WorkerProtocolError> {
    response.validate()?;
    encode_payload(response)
}

/// Decodes one unframed worker response payload.
///
/// # Errors
///
/// Returns [`WorkerProtocolError`] for malformed JSON or an invalid response.
pub fn decode_worker_response_payload(
    payload: &[u8],
) -> Result<WorkerResponse, WorkerProtocolError> {
    let response: WorkerResponse = decode_payload(payload)?;
    response.validate()?;
    Ok(response)
}

impl crate::SupervisedWorkerResponse for WorkerResponse {
    fn evaluation_policy_receipt(&self) -> Option<&EvaluationPolicyReceipt> {
        match self {
            Self::Success { result, .. } => Some(&result.evaluation),
            Self::Failure { failure, .. } => failure.evaluation.as_ref(),
        }
    }
}

fn encode_frame<T>(value: &T) -> Result<Vec<u8>, WorkerProtocolError>
where
    T: Serialize,
{
    let payload = encode_payload(value)?;
    let payload_length =
        u32::try_from(payload.len()).map_err(|_| WorkerProtocolError::FrameTooLarge {
            actual: u64::MAX,
            maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
        })?;
    if payload_length > WORKER_PROTOCOL_MAX_PAYLOAD_BYTES {
        return Err(WorkerProtocolError::FrameTooLarge {
            actual: u64::from(payload_length),
            maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
        });
    }
    let mut frame =
        Vec::with_capacity(WORKER_PROTOCOL_LENGTH_PREFIX_BYTES.saturating_add(payload.len()));
    frame.extend_from_slice(&payload_length.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

fn encode_payload<T>(value: &T) -> Result<Vec<u8>, WorkerProtocolError>
where
    T: Serialize,
{
    let payload = serde_json::to_vec(value).map_err(|error| WorkerProtocolError::Encoding {
        details: error.to_string(),
    })?;
    let payload_length =
        u32::try_from(payload.len()).map_err(|_| WorkerProtocolError::FrameTooLarge {
            actual: u64::MAX,
            maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
        })?;
    if payload_length > WORKER_PROTOCOL_MAX_PAYLOAD_BYTES {
        return Err(WorkerProtocolError::FrameTooLarge {
            actual: u64::from(payload_length),
            maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
        });
    }
    Ok(payload)
}

fn decode_payload<T>(payload: &[u8]) -> Result<T, WorkerProtocolError>
where
    T: DeserializeOwned,
{
    let actual = u64::try_from(payload.len()).unwrap_or(u64::MAX);
    if actual > u64::from(WORKER_PROTOCOL_MAX_PAYLOAD_BYTES) {
        return Err(WorkerProtocolError::FrameTooLarge {
            actual,
            maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
        });
    }
    serde_json::from_slice(payload).map_err(|error| WorkerProtocolError::Decoding {
        details: error.to_string(),
    })
}

fn decode_frame<T>(frame: &[u8]) -> Result<T, WorkerProtocolError>
where
    T: DeserializeOwned,
{
    let header = frame.get(..WORKER_PROTOCOL_LENGTH_PREFIX_BYTES).ok_or(
        WorkerProtocolError::TruncatedHeader {
            actual: frame.len(),
            required: WORKER_PROTOCOL_LENGTH_PREFIX_BYTES,
        },
    )?;
    let header: [u8; WORKER_PROTOCOL_LENGTH_PREFIX_BYTES] =
        header
            .try_into()
            .map_err(|_| WorkerProtocolError::TruncatedHeader {
                actual: frame.len(),
                required: WORKER_PROTOCOL_LENGTH_PREFIX_BYTES,
            })?;
    let declared = u32::from_be_bytes(header);
    if declared > WORKER_PROTOCOL_MAX_PAYLOAD_BYTES {
        return Err(WorkerProtocolError::FrameTooLarge {
            actual: u64::from(declared),
            maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
        });
    }
    let payload = &frame[WORKER_PROTOCOL_LENGTH_PREFIX_BYTES..];
    let declared = usize::try_from(declared).map_err(|_| WorkerProtocolError::FrameTooLarge {
        actual: u64::MAX,
        maximum: WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
    })?;
    if payload.len() < declared {
        return Err(WorkerProtocolError::TruncatedPayload {
            declared,
            actual: payload.len(),
        });
    }
    if payload.len() > declared {
        return Err(WorkerProtocolError::TrailingBytes {
            declared,
            trailing: payload.len() - declared,
        });
    }
    serde_json::from_slice(payload).map_err(|error| WorkerProtocolError::Decoding {
        details: error.to_string(),
    })
}

fn validate_version(
    axis: &'static str,
    found: u32,
    supported: u32,
) -> Result<(), WorkerProtocolError> {
    if found == supported {
        Ok(())
    } else {
        Err(WorkerProtocolError::UnsupportedVersion {
            axis,
            found,
            supported,
        })
    }
}

fn validate_diagnostics(
    diagnostics: &[Diagnostic],
    evaluation: &EvaluationPolicyReceipt,
) -> Result<(), WorkerProtocolError> {
    let limit = usize::try_from(evaluation.limits.retained_diagnostics).map_err(|_| {
        WorkerProtocolError::InvalidResponse {
            details: "diagnostic limit exceeds host representation".to_owned(),
        }
    })?;
    validate_diagnostics_with_limit(diagnostics, limit)
}

fn validate_diagnostics_with_limit(
    diagnostics: &[Diagnostic],
    limit: usize,
) -> Result<(), WorkerProtocolError> {
    if diagnostics.len() > limit {
        return Err(WorkerProtocolError::InvalidResponse {
            details: format!(
                "diagnostic count {} exceeds retained limit {limit}",
                diagnostics.len()
            ),
        });
    }
    let normalized = normalize_diagnostics(diagnostics.to_vec(), limit).map_err(|error| {
        WorkerProtocolError::InvalidResponse {
            details: format!("diagnostic normalization failed: {error}"),
        }
    })?;
    if normalized != diagnostics {
        return Err(WorkerProtocolError::InvalidResponse {
            details: "diagnostics are not in canonical bounded form".to_owned(),
        });
    }
    Ok(())
}

/// Stable framing, versioning, or atomic-result protocol failure.
#[derive(Clone, Debug, Eq, Error, PartialEq)]
pub enum WorkerProtocolError {
    /// A frame did not contain the complete four-byte length prefix.
    #[error("worker protocol frame header is truncated: got {actual} bytes, need {required}")]
    TruncatedHeader {
        /// Available frame bytes.
        actual: usize,
        /// Required header bytes.
        required: usize,
    },
    /// A declared or encoded payload exceeded the fixed protocol maximum.
    #[error("worker protocol payload {actual} bytes exceeds maximum {maximum}")]
    FrameTooLarge {
        /// Declared or encoded payload bytes.
        actual: u64,
        /// Fixed inclusive payload maximum.
        maximum: u32,
    },
    /// A frame ended before its declared payload length.
    #[error("worker protocol payload is truncated: declared {declared} bytes, got {actual}")]
    TruncatedPayload {
        /// Declared payload bytes.
        declared: usize,
        /// Available payload bytes.
        actual: usize,
    },
    /// Bytes followed the one declared payload.
    #[error("worker protocol frame has {trailing} trailing bytes after {declared}-byte payload")]
    TrailingBytes {
        /// Declared payload bytes.
        declared: usize,
        /// Bytes following the declared payload.
        trailing: usize,
    },
    /// A typed protocol message could not be serialized.
    #[error("worker protocol encoding failed: {details}")]
    Encoding {
        /// Serializer diagnostic.
        details: String,
    },
    /// A payload was malformed or contained unknown fields or variants.
    #[error("worker protocol decoding failed: {details}")]
    Decoding {
        /// Deserializer diagnostic.
        details: String,
    },
    /// One explicit protocol or model version axis was unsupported.
    #[error("unsupported worker protocol {axis}: found {found}, supported {supported}")]
    UnsupportedVersion {
        /// Version field name.
        axis: &'static str,
        /// Received version.
        found: u32,
        /// Exact supported version.
        supported: u32,
    },
    /// A request violated source, target, or policy authority.
    #[error("invalid worker request: {details}")]
    InvalidRequest {
        /// Stable request validation context.
        details: String,
    },
    /// A response violated atomic result, receipt, or diagnostic invariants.
    #[error("invalid worker response: {details}")]
    InvalidResponse {
        /// Stable response validation context.
        details: String,
    },
    /// Canonical normalized composition bytes were invalid.
    #[error("invalid canonical composition output: {details}")]
    InvalidComposition {
        /// Stable typed-output validation context.
        details: String,
    },
    /// Canonical package bytes were invalid.
    #[error("invalid canonical package output: {details}")]
    InvalidPackage {
        /// Stable typed-output validation context.
        details: String,
    },
}

impl WorkerProtocolError {
    /// Returns the stable Lattice diagnostic code for every protocol failure.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        "compose.worker_protocol"
    }
}

#[cfg(test)]
mod protocol_tests {
    use std::fs;
    use std::path::Path;
    use std::str::FromStr;

    use latticeaxiom_core::{CanonicalHash, SourceId, SourceProvenance, StableId, TargetTriple};

    use super::*;
    use crate::{
        AuthorizedRoot, AuthorizedRootKind, EnforcementCapability, NickelEvaluationLimits,
        R0_LINUX_MEMORY_BACKEND, SourceAddress, SourceRootGrant, SourceScanLimits,
        scan_source_snapshot,
    };

    const ENTRY_SOURCE_ID: &str = "latticeaxiom:source/protocol-fixture";
    const ENTRY_PATH: &str = "entry.ncl";
    const ENTRY_SOURCE: &[u8] = b"{}";

    #[test]
    fn request_payload_and_frame_round_trip_exactly() {
        let fixture = RequestFixture::package();
        let frame = encode_worker_request(&fixture.request)
            .unwrap_or_else(|error| panic!("request frame should encode: {error}"));
        let decoded = decode_worker_request(&frame)
            .unwrap_or_else(|error| panic!("request frame should decode: {error}"));
        assert_eq!(decoded, fixture.request);

        let declared = u32::from_be_bytes(
            frame[..WORKER_PROTOCOL_LENGTH_PREFIX_BYTES]
                .try_into()
                .unwrap_or_else(|error| panic!("frame prefix should be complete: {error}")),
        );
        assert_eq!(
            usize::try_from(declared)
                .unwrap_or_else(|error| panic!("frame length should be portable: {error}")),
            frame.len() - WORKER_PROTOCOL_LENGTH_PREFIX_BYTES
        );
    }

    #[test]
    fn request_rejects_unsorted_snapshots_and_unknown_fields() {
        let fixture = RequestFixture::package();
        let mut request = fixture.request.clone();
        let first_snapshot = request
            .snapshots
            .first()
            .cloned()
            .unwrap_or_else(|| panic!("fixture should contain its entry snapshot"));
        let second_source_id = SourceId::from_str("latticeaxiom:source/zz-protocol-fixture")
            .unwrap_or_else(|error| panic!("second source ID should be valid: {error}"));
        let second_snapshot = snapshot_with_source_id(&first_snapshot, &second_source_id);
        let second_entry = SourceAddress::new(second_source_id.clone(), ENTRY_PATH)
            .unwrap_or_else(|error| panic!("second entry should be valid: {error}"));
        request.source_closure.root_grants.push(SourceRootGrant {
            source_id: second_source_id,
            root_kind: second_snapshot.root_kind(),
            source_hash: second_snapshot.source_hash(),
            entry: second_entry,
            package_alias: None,
        });
        request
            .source_closure
            .root_grants
            .sort_by(|left, right| left.source_id.cmp(&right.source_id));
        request.snapshots = vec![second_snapshot, first_snapshot];
        assert!(matches!(
            request.validate(),
            Err(WorkerProtocolError::InvalidRequest { .. })
        ));

        request
            .snapshots
            .sort_by(|left, right| left.source_id().cmp(right.source_id()));
        request
            .validate()
            .unwrap_or_else(|error| panic!("canonically sorted snapshots should pass: {error}"));

        let mut payload = serde_json::to_value(&request)
            .unwrap_or_else(|error| panic!("request payload should serialize: {error}"));
        let serde_json::Value::Object(fields) = &mut payload else {
            panic!("request payload should be an object");
        };
        fields.insert("ambient_imports".to_owned(), serde_json::Value::Null);
        let frame = encode_frame(&payload)
            .unwrap_or_else(|error| panic!("unchecked payload frame should encode: {error}"));
        assert!(matches!(
            decode_worker_request(&frame),
            Err(WorkerProtocolError::Decoding { .. })
        ));
    }

    #[test]
    fn game_profile_request_binds_entry_provenance() {
        let fixture = RequestFixture::game_profile();
        fixture
            .request
            .validate()
            .unwrap_or_else(|error| panic!("matching entry provenance should pass: {error}"));

        let mut mismatched = fixture.request.clone();
        let AuthoringTarget::GameProfile { provenance, .. } = &mut mismatched.authoring_target
        else {
            panic!("fixture should be a game-profile request");
        };
        **provenance = fixture_provenance(b"different source");
        assert!(matches!(
            mismatched.validate(),
            Err(WorkerProtocolError::InvalidRequest { .. })
        ));
    }

    #[test]
    fn failure_wire_shape_has_no_partial_output_and_rejects_injection() {
        let response = WorkerResponse::Failure {
            versions: WorkerProtocolVersions::current(),
            failure: WorkerFailure {
                diagnostics: vec![failure_diagnostic()],
                source_closure: None,
                evaluation: None,
            },
        };
        let frame = encode_worker_response(&response)
            .unwrap_or_else(|error| panic!("atomic failure should encode: {error}"));
        let decoded = decode_worker_response(&frame)
            .unwrap_or_else(|error| panic!("atomic failure should decode: {error}"));
        assert_eq!(decoded, response);

        let mut payload = serde_json::to_value(&response)
            .unwrap_or_else(|error| panic!("failure payload should serialize: {error}"));
        let serde_json::Value::Object(envelope) = &mut payload else {
            panic!("response envelope should be an object");
        };
        let Some(serde_json::Value::Object(variant)) = envelope.get_mut("payload") else {
            panic!("response payload should be an object");
        };
        let Some(serde_json::Value::Object(failure)) = variant.get_mut("failure") else {
            panic!("failure result should be an object");
        };
        assert!(!failure.contains_key("output"));
        assert!(!failure.contains_key("canonical_hash"));
        assert!(!failure.contains_key("canonical_bytes"));

        failure.insert("output".to_owned(), serde_json::Value::Null);
        let injected = encode_frame(&payload)
            .unwrap_or_else(|error| panic!("injected payload frame should encode: {error}"));
        assert!(matches!(
            decode_worker_response(&injected),
            Err(WorkerProtocolError::Decoding { .. })
        ));
    }

    #[test]
    fn one_slot_truncation_summary_is_a_valid_atomic_failure() {
        let mut second = failure_diagnostic();
        second.summary = "second evaluation failure".to_owned();
        let diagnostics = normalize_diagnostics(vec![failure_diagnostic(), second], 1)
            .unwrap_or_else(|error| panic!("one-slot diagnostics should normalize: {error}"));
        let mut evaluation = developer_policy();
        evaluation.limits.retained_diagnostics = 1;
        let response = WorkerResponse::Failure {
            versions: WorkerProtocolVersions::current(),
            failure: WorkerFailure {
                diagnostics,
                source_closure: None,
                evaluation: Some(evaluation),
            },
        };

        let frame = encode_worker_response(&response)
            .unwrap_or_else(|error| panic!("one-slot failure should encode: {error}"));
        let decoded = decode_worker_response(&frame)
            .unwrap_or_else(|error| panic!("one-slot failure should decode: {error}"));
        assert_eq!(decoded, response);

        let mut malformed = response;
        let WorkerResponse::Failure { failure, .. } = &mut malformed else {
            panic!("fixture should remain an atomic failure");
        };
        let diagnostic = failure
            .diagnostics
            .first_mut()
            .unwrap_or_else(|| panic!("truncation summary should be retained"));
        diagnostic.notes = vec!["total=2; retained=0; omitted=1".to_owned()];
        assert!(matches!(
            malformed.validate(),
            Err(WorkerProtocolError::InvalidResponse { .. })
        ));
    }

    #[cfg(feature = "nickel-evaluator")]
    #[test]
    fn standalone_failure_rejects_closure_meters_beyond_evaluation_limits() {
        let fixture = RequestFixture::package();
        let snapshots = fixture
            .request
            .snapshots
            .iter()
            .cloned()
            .map(|snapshot| (snapshot.source_id().clone(), snapshot))
            .collect::<BTreeMap<_, _>>();
        let closure =
            crate::resolve_nickel_source_closure(&snapshots, &fixture.request.source_closure)
                .unwrap_or_else(|error| panic!("fixture closure should resolve: {error}"));
        assert!(closure.source_bytes > 1);

        let mut evaluation = fixture.request.expected_policy.clone();
        evaluation.limits.source_bytes = 1;
        let response = WorkerResponse::Failure {
            versions: WorkerProtocolVersions::current(),
            failure: WorkerFailure {
                diagnostics: vec![failure_diagnostic()],
                source_closure: Some(closure),
                evaluation: Some(evaluation),
            },
        };

        let error = response
            .validate()
            .err()
            .unwrap_or_else(|| panic!("oversized failure closure unexpectedly validated"));
        assert!(matches!(
            error,
            WorkerProtocolError::InvalidResponse { details }
                if details == "failure source closure meters exceed evaluation limits"
        ));
    }

    #[cfg(feature = "nickel-evaluator")]
    #[test]
    fn success_binds_exact_closure_and_composition_provenance() {
        let fixture = RequestFixture::game_profile();
        let snapshots = fixture
            .request
            .snapshots
            .iter()
            .cloned()
            .map(|snapshot| (snapshot.source_id().clone(), snapshot))
            .collect::<BTreeMap<_, _>>();
        let closure =
            crate::resolve_nickel_source_closure(&snapshots, &fixture.request.source_closure)
                .unwrap_or_else(|error| panic!("fixture closure should resolve: {error}"));

        let mut composition = serde_json::from_slice::<CompositionSpec>(include_bytes!(
            "../../../fixtures/composition/positive/headless-composition.golden.json"
        ))
        .unwrap_or_else(|error| panic!("composition fixture should decode: {error}"));
        composition.provenance = fixture_provenance(ENTRY_SOURCE);
        composition.policy.target = fixture.request.expected_policy.target.clone();
        composition.policy.evaluation_policy = fixture.request.expected_policy.policy.clone();
        composition.policy.evaluation_limits = fixture.request.expected_policy.limits;
        let output = CanonicalCompositionSpec::from_spec(&composition)
            .unwrap_or_else(|error| panic!("composition fixture should canonicalize: {error}"));
        let response = WorkerResponse::Success {
            versions: WorkerProtocolVersions::current(),
            result: WorkerSuccess {
                output: WorkerOutput::Composition(output),
                source_closure: closure.clone(),
                diagnostics: Vec::new(),
                evaluation: fixture.request.expected_policy.clone(),
            },
        };
        response
            .validate_against_request(&fixture.request)
            .unwrap_or_else(|error| panic!("exact success should match request: {error}"));

        composition.provenance = fixture_provenance(b"different source");
        let mismatched_output = CanonicalCompositionSpec::from_spec(&composition)
            .unwrap_or_else(|error| panic!("mismatched composition should remain typed: {error}"));
        let mismatched = WorkerResponse::Success {
            versions: WorkerProtocolVersions::current(),
            result: WorkerSuccess {
                output: WorkerOutput::Composition(mismatched_output),
                source_closure: closure,
                diagnostics: Vec::new(),
                evaluation: fixture.request.expected_policy.clone(),
            },
        };
        assert!(matches!(
            mismatched.validate_against_request(&fixture.request),
            Err(WorkerProtocolError::InvalidResponse { .. })
        ));
    }

    fn snapshot_with_source_id(snapshot: &SourceSnapshot, source_id: &SourceId) -> SourceSnapshot {
        let mut value = serde_json::to_value(snapshot)
            .unwrap_or_else(|error| panic!("snapshot should serialize: {error}"));
        let serde_json::Value::Object(fields) = &mut value else {
            panic!("snapshot payload should be an object");
        };
        fields.insert(
            "source_id".to_owned(),
            serde_json::Value::String(source_id.to_string()),
        );
        serde_json::from_value(value)
            .unwrap_or_else(|error| panic!("snapshot clone should deserialize: {error}"))
    }

    fn failure_diagnostic() -> Diagnostic {
        Diagnostic {
            code: "compose.evaluation_failed"
                .parse()
                .unwrap_or_else(|error| panic!("fixture diagnostic code should be valid: {error}")),
            severity: DiagnosticSeverity::Error,
            summary: "evaluation failed".to_owned(),
            labels: Vec::new(),
            notes: Vec::new(),
        }
    }

    fn fixture_provenance(source: &[u8]) -> SourceProvenance {
        SourceProvenance::new(
            SourceId::from_str(ENTRY_SOURCE_ID)
                .unwrap_or_else(|error| panic!("fixture source ID should be valid: {error}")),
            ENTRY_PATH,
            CanonicalHash::digest(source),
            None,
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("fixture provenance should be valid: {error}"))
    }

    fn developer_policy() -> EvaluationPolicyReceipt {
        EvaluationPolicyReceipt {
            policy: stable_id("latticeaxiom:nickel-evaluation-policy/developer@1"),
            target: target("x86_64-pc-windows-msvc"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::unsupported(),
            memory: EnforcementCapability::unsupported(),
            recursion: EnforcementCapability::unsupported(),
        }
    }

    fn production_policy() -> EvaluationPolicyReceipt {
        EvaluationPolicyReceipt {
            policy: stable_id(crate::R0_NICKEL_EVALUATION_POLICY),
            target: target("x86_64-unknown-linux-gnu"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-deadline-backend/supervisor-monotonic@1",
            )),
            memory: EnforcementCapability::hard(stable_id(R0_LINUX_MEMORY_BACKEND)),
            recursion: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-recursion-backend/nickel-vm-frames@1",
            )),
        }
    }

    fn stable_id(value: &str) -> StableId {
        StableId::from_str(value)
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` should be valid: {error}"))
    }

    fn target(value: &str) -> TargetTriple {
        TargetTriple::from_str(value)
            .unwrap_or_else(|error| panic!("fixture target `{value}` should be valid: {error}"))
    }

    struct RequestFixture {
        _directory: TestDirectory,
        request: WorkerRequest,
    }

    impl RequestFixture {
        fn package() -> Self {
            Self::new(false)
        }

        fn game_profile() -> Self {
            Self::new(true)
        }

        fn new(game_profile: bool) -> Self {
            let directory = TestDirectory::new();
            fs::write(directory.path().join(ENTRY_PATH), ENTRY_SOURCE)
                .unwrap_or_else(|error| panic!("entry fixture write should succeed: {error}"));
            let source_id = SourceId::from_str(ENTRY_SOURCE_ID)
                .unwrap_or_else(|error| panic!("fixture source ID should be valid: {error}"));
            let root = AuthorizedRoot::new(
                source_id.clone(),
                AuthorizedRootKind::Test,
                directory.path(),
            )
            .unwrap_or_else(|error| panic!("fixture root should be valid: {error}"));
            let snapshot = scan_source_snapshot(
                &root,
                SourceScanLimits {
                    maximum_files: 8,
                    maximum_bytes: 1_024,
                },
            )
            .unwrap_or_else(|error| panic!("fixture snapshot should scan: {error}"));
            let entry = SourceAddress::new(source_id.clone(), ENTRY_PATH)
                .unwrap_or_else(|error| panic!("fixture entry should be valid: {error}"));
            let provenance = fixture_provenance(ENTRY_SOURCE);
            let policy = if game_profile {
                production_policy()
            } else {
                developer_policy()
            };
            let authoring_target = if game_profile {
                AuthoringTarget::GameProfile {
                    composition_target: policy.target.clone(),
                    provenance: Box::new(provenance.clone()),
                }
            } else {
                AuthoringTarget::Package
            };
            let request = WorkerRequest {
                versions: WorkerProtocolVersions::current(),
                authoring_target,
                worker_target: policy.target.clone(),
                execution_class: WorkerExecutionClass::TrustedFixture,
                source_closure: SourceClosureRequest {
                    library_contract_major: NICKEL_LIBRARY_CONTRACT_MAJOR,
                    corpus_major: R0_AUTHORING_CORPUS_MAJOR,
                    entry: entry.clone(),
                    root_grants: vec![SourceRootGrant {
                        source_id,
                        root_kind: AuthorizedRootKind::Test,
                        source_hash: snapshot.source_hash(),
                        entry,
                        package_alias: None,
                    }],
                    package_instances: BTreeMap::new(),
                    alias_edges: BTreeMap::new(),
                    limits: policy.limits,
                },
                snapshots: vec![snapshot],
                expected_policy: policy,
            };
            Self {
                _directory: directory,
                request,
            }
        }
    }

    struct TestDirectory {
        directory: tempfile::TempDir,
    }

    impl TestDirectory {
        fn new() -> Self {
            let directory = tempfile::Builder::new()
                .prefix("latticeaxiom-evaluator-protocol-")
                .tempdir()
                .unwrap_or_else(|error| panic!("fixture directory should be created: {error}"));
            Self { directory }
        }

        fn path(&self) -> &Path {
            self.directory.path()
        }
    }
}
