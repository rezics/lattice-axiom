//! Pure worker-side handling for one typed Nickel evaluation request.
//!
//! The handler in this module performs no process I/O and never reads the
//! ambient filesystem. It revalidates the protocol request, recomputes the
//! static source closure from immutable snapshot bytes with the fixed Nickel
//! scanner, and returns either one complete typed output or a failure without
//! any partial package or composition.

use std::str::FromStr;

use crate::{
    Diagnostic, DiagnosticCode, DiagnosticCodeError, DiagnosticPolicyError, DiagnosticSeverity,
    EnforcementCapability, EnforcementStrength, EvaluationPolicyReceipt,
    R0_NICKEL_EVALUATION_POLICY, R0_SUPERVISOR_DEADLINE_BACKEND, WORKER_PROTOCOL_MAX_DIAGNOSTICS,
    WorkerExecutionClass, WorkerFailure, WorkerProtocolError, WorkerProtocolVersions,
    WorkerRequest, WorkerResponse, normalize_diagnostics,
};
use thiserror::Error;

#[cfg(feature = "nickel-evaluator")]
use crate::{
    AuthoringTarget, CanonicalCompositionSpec, CanonicalPackageSpec, GameProfileSpec,
    PackageDomain, PackageSpec, ProfileKind, WorkerOutput, WorkerSuccess,
};

/// Internal construction failure for a worker response.
///
/// Authored-input, protocol, policy, source, and typed-schema failures are
/// returned as [`WorkerResponse::Failure`]. This error is reserved for broken
/// built-in diagnostic literals or deterministic diagnostic normalization,
/// both of which indicate a worker implementation defect.
#[derive(Debug, Error)]
pub enum WorkerHandlerError {
    /// A built-in diagnostic literal violated stable code grammar.
    #[error("invalid built-in worker diagnostic code `{code}`: {source}")]
    InvalidDiagnosticCode {
        /// Invalid built-in literal.
        code: &'static str,
        /// Stable-code validation failure.
        #[source]
        source: DiagnosticCodeError,
    },
    /// Deterministic diagnostic normalization failed.
    #[error("worker diagnostic normalization failed: {0}")]
    DiagnosticPolicy(#[from] DiagnosticPolicyError),
}

/// Handles one complete worker request without process or filesystem I/O.
///
/// Request versions, policy, limits, grants, and immutable snapshots are
/// revalidated even for an in-process caller. With Nickel support enabled, the
/// handler recomputes the source closure with the fixed Nickel 0.18 scanner.
/// The current trusted evaluator has no source-table-only loader, so it runs
/// only an import-free entry under an explicit trusted-tool or trusted-fixture
/// class and a non-R0 policy that truthfully reports the controller's hard
/// monotonic deadline while leaving memory and recursion unsupported.
/// Production `r0@1` stays fail-closed until a loader can consume snapshot
/// bytes together with lock-scoped alias edges.
///
/// The production `r0@1` policy always fails closed with
/// `compose.worker_capability`: this pure handler does not install the
/// source-table-only loader, hard memory containment, or Nickel call-frame
/// recursion meter. A controller-provided hard receipt is never treated as
/// proof that those capabilities exist inside this handler.
///
/// # Errors
///
/// Returns [`WorkerHandlerError`] only for an invalid built-in diagnostic code
/// or deterministic diagnostic-normalization defect. Every expected request,
/// policy, source, evaluation, capability, or schema failure is represented by
/// an atomic [`WorkerResponse::Failure`].
pub fn handle_worker_request(request: WorkerRequest) -> Result<WorkerResponse, WorkerHandlerError> {
    if let Err(error) = request.validate() {
        return failure_response(
            "compose.worker_protocol",
            "worker request validation failed",
            error.to_string(),
            None,
            None,
        );
    }

    if let Err(details) = validate_execution_authority(&request) {
        return failure_response(
            "compose.policy_violation",
            "worker execution class is not authorized",
            details,
            None,
            None,
        );
    }

    if request.expected_policy.policy.as_str() == R0_NICKEL_EVALUATION_POLICY {
        return failure_response(
            "compose.worker_capability",
            "production Nickel evaluation capabilities are unavailable",
            "the source-table-only loader, hard memory containment, and Nickel call-frame recursion meter are not installed in this pure handler".to_owned(),
            None,
            None,
        );
    }

    if !accurately_reports_controller_enforced_adapter(&request.expected_policy) {
        return failure_response(
            "compose.worker_capability",
            "trusted Nickel adapter capabilities were overstated",
            "the trusted adapter must report the supervisor monotonic deadline as hard and memory and recursion as unsupported".to_owned(),
            None,
            None,
        );
    }

    #[cfg(not(feature = "nickel-evaluator"))]
    {
        failure_response(
            "compose.worker_capability",
            "Nickel evaluation support is unavailable",
            "the latticeaxiom-compose crate was built without the nickel-evaluator feature"
                .to_owned(),
            None,
            Some(request.expected_policy),
        )
    }

    #[cfg(feature = "nickel-evaluator")]
    {
        handle_with_nickel(request)
    }
}

fn validate_execution_authority(request: &WorkerRequest) -> Result<(), String> {
    if request.execution_class != WorkerExecutionClass::TrustedFixture {
        return Ok(());
    }
    let entry_source = request.source_closure.entry.source_id();
    let entry_grant = request
        .source_closure
        .root_grants
        .iter()
        .find(|grant| &grant.source_id == entry_source)
        .ok_or_else(|| format!("entry source root `{entry_source}` is not granted"))?;
    if entry_grant.root_kind != crate::AuthorizedRootKind::Test {
        return Err(format!(
            "trusted fixture entry `{entry_source}` must use a test-fixture root"
        ));
    }
    Ok(())
}

fn accurately_reports_controller_enforced_adapter(receipt: &EvaluationPolicyReceipt) -> bool {
    let unsupported = EnforcementCapability::unsupported();
    receipt.deadline.strength == EnforcementStrength::Hard
        && receipt
            .deadline
            .backend
            .as_ref()
            .map(latticeaxiom_core::StableId::as_str)
            == Some(R0_SUPERVISOR_DEADLINE_BACKEND)
        && receipt.memory == unsupported
        && receipt.recursion == unsupported
}

#[cfg(feature = "nickel-evaluator")]
fn handle_with_nickel(request: WorkerRequest) -> Result<WorkerResponse, WorkerHandlerError> {
    let snapshots = request
        .snapshots
        .iter()
        .cloned()
        .map(|snapshot| (snapshot.source_id().clone(), snapshot))
        .collect::<std::collections::BTreeMap<_, _>>();
    let closure = match crate::resolve_nickel_source_closure(&snapshots, &request.source_closure) {
        Ok(receipt) => receipt,
        Err(error) => {
            return failure_response(
                error.code(),
                "Nickel source closure preflight failed",
                error.to_string(),
                None,
                Some(request.expected_policy),
            );
        }
    };

    if closure.sources.len() != 1 || !closure.edges.is_empty() {
        return failure_response(
            "compose.worker_capability",
            "source-table-only Nickel loading is unavailable",
            "the temporary trusted adapter accepts only import-free entries and never falls back to ambient filesystem resolution".to_owned(),
            Some(closure),
            Some(request.expected_policy),
        );
    }

    let entry = &request.source_closure.entry;
    let Some(entry_snapshot) = snapshots.get(entry.source_id()) else {
        return failure_response(
            "compose.worker_protocol",
            "worker entry snapshot disappeared after validation",
            entry.to_string(),
            Some(closure),
            Some(request.expected_policy),
        );
    };
    let Some(entry_file) = entry_snapshot.files().get(entry.logical_path()) else {
        return failure_response(
            "compose.worker_protocol",
            "worker entry source disappeared after validation",
            entry.to_string(),
            Some(closure),
            Some(request.expected_policy),
        );
    };
    let source = match std::str::from_utf8(entry_file.bytes()) {
        Ok(source) => source,
        Err(error) => {
            return failure_response(
                "compose.evaluation_failed",
                "Nickel entry source is not UTF-8",
                error.to_string(),
                Some(closure),
                Some(request.expected_policy),
            );
        }
    };

    let output = match evaluate_typed_output(source, &request) {
        Ok(output) => output,
        Err(error) => {
            return failure_response(
                error.code,
                error.summary,
                error.note,
                Some(closure),
                Some(request.expected_policy),
            );
        }
    };

    Ok(WorkerResponse::Success {
        versions: WorkerProtocolVersions::current(),
        result: WorkerSuccess {
            output,
            source_closure: closure,
            diagnostics: Vec::new(),
            evaluation: request.expected_policy,
        },
    })
}

#[cfg(feature = "nickel-evaluator")]
fn evaluate_typed_output(
    source: &str,
    request: &WorkerRequest,
) -> Result<WorkerOutput, WorkerExecutionFailure> {
    match &request.authoring_target {
        AuthoringTarget::Package => {
            let mut package = crate::evaluate_trusted_nickel_source::<PackageSpec>(
                source,
                request.source_closure.entry.to_string(),
                request.expected_policy.limits,
            )
            .map_err(|error| WorkerExecutionFailure::evaluation(&error))?;
            package.provenance = entry_provenance(request)?;
            package
                .validate()
                .map_err(|error| WorkerExecutionFailure::schema(&error))?;
            validate_package_execution_class(&package, request.execution_class)?;
            let canonical = CanonicalPackageSpec::from_spec(&package)
                .map_err(|error| WorkerExecutionFailure::protocol_output(&error))?;
            ensure_output_limit(
                canonical.canonical_bytes.len(),
                request.expected_policy.limits.output_bytes,
            )?;
            Ok(WorkerOutput::Package(canonical))
        }
        AuthoringTarget::GameProfile {
            composition_target,
            provenance,
        } => {
            let profile = crate::evaluate_trusted_nickel_source::<GameProfileSpec>(
                source,
                request.source_closure.entry.to_string(),
                request.expected_policy.limits,
            )
            .map_err(|error| WorkerExecutionFailure::evaluation(&error))?;
            validate_profile_execution_class(&profile, request.execution_class)?;
            if profile.evaluation_policy != request.expected_policy.policy {
                return Err(WorkerExecutionFailure::policy(
                    "evaluated profile policy differs from the worker request",
                ));
            }
            if profile.evaluation_limits != request.expected_policy.limits {
                return Err(WorkerExecutionFailure::policy(
                    "evaluated profile limits differ from the worker request",
                ));
            }
            let composition = profile
                .into_composition(composition_target.clone(), provenance.as_ref().clone())
                .map_err(|error| WorkerExecutionFailure::schema(&error))?;
            let canonical = CanonicalCompositionSpec::from_spec(&composition)
                .map_err(|error| WorkerExecutionFailure::protocol_output(&error))?;
            ensure_output_limit(
                canonical.canonical_bytes.len(),
                request.expected_policy.limits.output_bytes,
            )?;
            Ok(WorkerOutput::Composition(canonical))
        }
    }
}

#[cfg(feature = "nickel-evaluator")]
fn entry_provenance(
    request: &WorkerRequest,
) -> Result<latticeaxiom_core::SourceProvenance, WorkerExecutionFailure> {
    let entry = &request.source_closure.entry;
    let snapshot = request
        .snapshots
        .iter()
        .find(|snapshot| snapshot.source_id() == entry.source_id())
        .ok_or_else(|| WorkerExecutionFailure::source_provenance("entry snapshot is absent"))?;
    let file = snapshot
        .files()
        .get(entry.logical_path())
        .ok_or_else(|| WorkerExecutionFailure::source_provenance("entry source is absent"))?;
    latticeaxiom_core::SourceProvenance::new(
        entry.source_id().clone(),
        entry.logical_path(),
        file.receipt().content_hash(),
        None,
        Vec::new(),
    )
    .map_err(|error| WorkerExecutionFailure::source_provenance(error.to_string()))
}

#[cfg(feature = "nickel-evaluator")]
fn validate_package_execution_class(
    package: &PackageSpec,
    execution_class: WorkerExecutionClass,
) -> Result<(), WorkerExecutionFailure> {
    if execution_class == WorkerExecutionClass::TrustedTool
        && package.domains != std::collections::BTreeSet::from([PackageDomain::Tool])
    {
        return Err(WorkerExecutionFailure::policy(
            "trusted-tool package output must use exactly the tool domain",
        ));
    }
    Ok(())
}

#[cfg(feature = "nickel-evaluator")]
fn validate_profile_execution_class(
    profile: &GameProfileSpec,
    execution_class: WorkerExecutionClass,
) -> Result<(), WorkerExecutionFailure> {
    if execution_class == WorkerExecutionClass::TrustedTool
        && profile.projection != ProfileKind::Tool
    {
        return Err(WorkerExecutionFailure::policy(
            "trusted-tool profile output must use the tool projection",
        ));
    }
    Ok(())
}

#[cfg(feature = "nickel-evaluator")]
fn ensure_output_limit(
    actual_bytes: usize,
    configured_bytes: u64,
) -> Result<(), WorkerExecutionFailure> {
    let maximum_bytes = usize::try_from(configured_bytes).map_err(|_| WorkerExecutionFailure {
        code: "compose.output_limit",
        summary: "configured output limit is unsupported",
        note: format!(
            "configured canonical output limit {configured_bytes} cannot be represented on this platform"
        ),
    })?;
    if actual_bytes > maximum_bytes {
        return Err(WorkerExecutionFailure {
            code: "compose.output_limit",
            summary: "normalized typed output exceeds policy",
            note: format!(
                "canonical typed output is {actual_bytes} bytes; limit is {maximum_bytes} bytes"
            ),
        });
    }
    Ok(())
}

#[cfg(feature = "nickel-evaluator")]
#[derive(Debug)]
struct WorkerExecutionFailure {
    code: &'static str,
    summary: &'static str,
    note: String,
}

#[cfg(feature = "nickel-evaluator")]
impl WorkerExecutionFailure {
    fn evaluation(error: &crate::NickelEvaluationError) -> Self {
        Self {
            code: error.code(),
            summary: "trusted Nickel evaluation failed",
            note: error.to_string(),
        }
    }

    fn schema(error: &crate::CompositionError) -> Self {
        Self {
            code: "compose.schema_mismatch",
            summary: "typed Nickel output failed normative validation",
            note: error.to_string(),
        }
    }

    fn policy(note: &'static str) -> Self {
        Self {
            code: "compose.policy_violation",
            summary: "trusted Nickel output violates its execution class",
            note: note.to_owned(),
        }
    }

    fn source_provenance(note: impl Into<String>) -> Self {
        Self {
            code: "compose.schema_mismatch",
            summary: "typed Nickel output source provenance is invalid",
            note: note.into(),
        }
    }

    fn protocol_output(error: &WorkerProtocolError) -> Self {
        Self {
            code: "compose.evaluation_failed",
            summary: "typed Nickel output could not be canonically encoded",
            note: error.to_string(),
        }
    }
}

fn failure_response(
    code: &'static str,
    summary: &'static str,
    note: String,
    source_closure: Option<crate::SourceClosureReceipt>,
    evaluation: Option<EvaluationPolicyReceipt>,
) -> Result<WorkerResponse, WorkerHandlerError> {
    let code = DiagnosticCode::from_str(code)
        .map_err(|source| WorkerHandlerError::InvalidDiagnosticCode { code, source })?;
    let diagnostics = normalize_diagnostics(
        vec![Diagnostic {
            code,
            severity: DiagnosticSeverity::Error,
            summary: summary.to_owned(),
            labels: Vec::new(),
            notes: vec![note],
        }],
        diagnostic_limit(evaluation.as_ref()),
    )?;
    Ok(WorkerResponse::Failure {
        versions: WorkerProtocolVersions::current(),
        failure: WorkerFailure {
            diagnostics,
            source_closure,
            evaluation,
        },
    })
}

fn diagnostic_limit(evaluation: Option<&EvaluationPolicyReceipt>) -> usize {
    evaluation
        .and_then(|receipt| usize::try_from(receipt.limits.retained_diagnostics).ok())
        .map_or(WORKER_PROTOCOL_MAX_DIAGNOSTICS, |limit| {
            limit.min(WORKER_PROTOCOL_MAX_DIAGNOSTICS)
        })
}

#[cfg(all(test, feature = "nickel-evaluator"))]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::str::FromStr;
    use std::sync::atomic::{AtomicU64, Ordering};

    use latticeaxiom_core::{
        CanonicalHash, PackageName, SourceId, SourceProvenance, StableId, TargetTriple,
    };

    use super::*;
    use crate::{
        AuthorizedRoot, AuthorizedRootKind, NickelEvaluationLimits, R0_WINDOWS_MEMORY_BACKEND,
        SourceAddress, SourceRootGrant, SourceScanLimits, scan_source_snapshot,
    };

    const TOOL_PACKAGE: &str = r#"
    {
      model_version = 3,
      name = "worker-tool",
      version = "0.1.0",
      metadata = { display_name = "Worker tool", license = "MIT", documentation = null },
      features = {}, dependencies = {}, requires = {}, provides = {},
      realizations = {
        data = {
          id = "data", kind = "data", domains = ["tool"], targets = [],
          required_features = [], trust = "data-only", interfaces = {},
          registration_fragment = "59915c3d09fa528003f7dfdf0b8fa41875c3038a0637f395711aef516547815c",
          engine_build = null, artifact = { kind = "data-root", path = "data" }
        }
      },
      domains = ["tool"], parameters = {}, namespace_requests = [], trust = "data-only",
      registration = {
        registrations = [], semantics = [], settings = [], composition_parameters = [],
        metrics = [], info_items = [], inspect = [], visualizers = []
      },
      provenance = {
        source_id = "latticeaxiom:source/worker-fixture", logical_path = "entry.ncl",
        content_hash = "0000000000000000000000000000000000000000000000000000000000000000",
        span = null, origin_chain = [], producer = null, toolchain = null,
        generated_fragment = null
      }
    }
    "#;

    const TOOL_PROFILE: &str = r#"
    {
      model_version = 3,
      profile = "latticeaxiom:profile/worker-tool@1",
      projection = "tool", projection_domains = ["tool"], roots = {}, capabilities = {},
      source_universe = [], features = {}, parameters = {}, realization_policy = ["data"],
      semantic_bindings = {}, overlays = [],
      evaluation_policy = "latticeaxiom:nickel-evaluation-policy/developer@1",
      evaluation_limits = {
        wall_clock_ms = 10000, memory_bytes = 268435456, recursion_depth = 512,
        import_depth = 64, imported_files = 4096, source_bytes = 16777216,
        output_bytes = 16777216, retained_diagnostics = 256
      },
      policy = {
        namespace_grants = {}, maximum_trust = "data-only",
        allow_force_override = false, allow_recovery = false
      }
    }
    "#;

    #[test]
    fn production_r0_fails_closed_without_partial_output_or_receipts() {
        let fixture = fixture_request(
            TOOL_PROFILE,
            AuthoringTarget::GameProfile {
                composition_target: target("x86_64-pc-windows-msvc"),
                provenance: Box::new(fixture_provenance(TOOL_PROFILE.as_bytes())),
            },
        );
        let mut request = fixture.request;
        request.expected_policy = production_policy();
        request.source_closure.limits = request.expected_policy.limits;

        let response = handle_worker_request(request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        let WorkerResponse::Failure { failure, .. } = response else {
            panic!("production R0 unexpectedly evaluated");
        };
        assert_eq!(failure.diagnostics.len(), 1);
        assert_eq!(
            failure.diagnostics[0].code.as_str(),
            "compose.worker_capability"
        );
        assert!(failure.source_closure.is_none());
        assert!(failure.evaluation.is_none());
    }

    #[test]
    fn production_r0_stays_fail_closed_when_bytes_and_alias_edges_are_supplied() {
        let fixture = fixture_request(TOOL_PACKAGE, AuthoringTarget::Package);
        let mut request = fixture.request;
        let package = PackageName::from_str("worker-tool")
            .unwrap_or_else(|error| panic!("fixture package name is invalid: {error}"));
        request.source_closure.package_instances.insert(
            request.source_closure.entry.source_id().clone(),
            package.clone(),
        );
        request
            .source_closure
            .alias_edges
            .insert(package, BTreeMap::new());
        request.expected_policy = production_policy();
        request.source_closure.limits = request.expected_policy.limits;

        let response = handle_worker_request(request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        let WorkerResponse::Failure { failure, .. } = response else {
            panic!("production R0 evaluated after bytes and alias edges were supplied");
        };
        assert_eq!(
            failure.diagnostics[0].code.as_str(),
            "compose.worker_capability"
        );
        assert!(failure.source_closure.is_none());
        assert!(failure.evaluation.is_none());
    }

    #[test]
    fn package_alias_without_lock_edges_fails_before_evaluation() {
        let fixture = fixture_request("import blocks", AuthoringTarget::Package);
        let response = handle_worker_request(fixture.request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        let WorkerResponse::Failure { failure, .. } = response else {
            panic!("missing lock alias reached evaluation");
        };
        assert_eq!(
            failure.diagnostics[0].code.as_str(),
            "compose.import_denied"
        );
        assert!(
            failure.diagnostics[0]
                .notes
                .iter()
                .any(|note| note.contains("pkg://")),
            "missing alias must fail closed with a pkg:// span"
        );
        assert!(
            failure.source_closure.is_none(),
            "missing lock alias must fail during source-closure preflight"
        );
    }

    #[test]
    fn trusted_fixture_returns_a_complete_typed_package() {
        let fixture = fixture_request(TOOL_PACKAGE, AuthoringTarget::Package);
        let response = handle_worker_request(fixture.request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        let WorkerResponse::Success { result, .. } = response else {
            panic!("trusted package fixture did not return typed success");
        };
        let WorkerOutput::Package(canonical) = result.output else {
            panic!("package target returned the wrong typed output");
        };
        let package = canonical
            .decode(result.evaluation.limits.output_bytes)
            .unwrap_or_else(|error| panic!("typed package output was invalid: {error}"));
        assert_eq!(package.name.as_str(), "worker-tool");
        assert_eq!(result.source_closure.imported_files, 1);
        assert!(result.source_closure.edges.is_empty());
    }

    #[test]
    fn trusted_game_profile_normalizes_to_one_complete_composition() {
        let fixture = fixture_request(
            TOOL_PROFILE,
            AuthoringTarget::GameProfile {
                composition_target: target("thumbv8m.main-none-eabi"),
                provenance: Box::new(fixture_provenance(TOOL_PROFILE.as_bytes())),
            },
        );
        let response = handle_worker_request(fixture.request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        let WorkerResponse::Success { result, .. } = response else {
            panic!("trusted profile did not return typed success");
        };
        let WorkerOutput::Composition(canonical) = result.output else {
            panic!("profile target returned the wrong typed output");
        };
        let composition = canonical
            .decode(result.evaluation.limits.output_bytes)
            .unwrap_or_else(|error| panic!("normalized composition was invalid: {error}"));
        assert_eq!(composition.profile_kind, ProfileKind::Tool);
        assert_eq!(
            composition.policy.target.as_str(),
            "thumbv8m.main-none-eabi"
        );
    }

    #[test]
    fn invalid_protocol_is_an_atomic_protocol_failure() {
        let fixture = fixture_request(TOOL_PACKAGE, AuthoringTarget::Package);
        let mut request = fixture.request;
        request.versions.corpus_major = request.versions.corpus_major.saturating_add(1);
        let response = handle_worker_request(request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        assert_failure_code(&response, "compose.worker_protocol");
    }

    #[test]
    fn static_imports_are_scanned_but_not_evaluated_ambiently() {
        let fixture = fixture_request(
            "let dependency = import \"dependency.ncl\" in dependency",
            AuthoringTarget::Package,
        );
        fs::write(
            fixture.directory.path().join("dependency.ncl"),
            TOOL_PACKAGE,
        )
        .unwrap_or_else(|error| panic!("dependency fixture write failed: {error}"));
        let request = fixture.rescan();
        let response = handle_worker_request(request)
            .unwrap_or_else(|error| panic!("worker response construction failed: {error}"));
        assert_failure_code(&response, "compose.worker_capability");
        let WorkerResponse::Failure { failure, .. } = response else {
            panic!("importful fixture unexpectedly succeeded");
        };
        assert_eq!(
            failure
                .source_closure
                .as_ref()
                .map(|receipt| receipt.imported_files),
            Some(2)
        );
    }

    fn assert_failure_code(response: &WorkerResponse, expected: &str) {
        let WorkerResponse::Failure { failure, .. } = response else {
            panic!("expected worker failure response");
        };
        assert_eq!(failure.diagnostics.len(), 1);
        assert_eq!(failure.diagnostics[0].code.as_str(), expected);
    }

    fn fixture_request(source: &str, target: AuthoringTarget) -> FixtureRequest {
        let directory = TestDirectory::new();
        fs::write(directory.path().join("entry.ncl"), source)
            .unwrap_or_else(|error| panic!("entry fixture write failed: {error}"));
        let request = request_from_directory(&directory, target);
        FixtureRequest { directory, request }
    }

    fn request_from_directory(
        directory: &TestDirectory,
        authoring_target: AuthoringTarget,
    ) -> WorkerRequest {
        let source_id = SourceId::from_str("latticeaxiom:source/worker-fixture")
            .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}"));
        let root = AuthorizedRoot::new(
            source_id.clone(),
            AuthorizedRootKind::Test,
            directory.path(),
        )
        .unwrap_or_else(|error| panic!("fixture root is invalid: {error}"));
        let snapshot = scan_source_snapshot(
            &root,
            SourceScanLimits {
                maximum_files: 16,
                maximum_bytes: 1_048_576,
            },
        )
        .unwrap_or_else(|error| panic!("fixture snapshot failed: {error}"));
        let entry = SourceAddress::new(source_id.clone(), "entry.ncl")
            .unwrap_or_else(|error| panic!("fixture entry is invalid: {error}"));
        let policy = trusted_policy();
        WorkerRequest {
            versions: WorkerProtocolVersions::current(),
            authoring_target,
            worker_target: policy.target.clone(),
            execution_class: WorkerExecutionClass::TrustedFixture,
            source_closure: crate::SourceClosureRequest {
                library_contract_major: crate::NICKEL_LIBRARY_CONTRACT_MAJOR,
                corpus_major: crate::R0_AUTHORING_CORPUS_MAJOR,
                entry: entry.clone(),
                root_grants: vec![SourceRootGrant {
                    source_id: source_id.clone(),
                    root_kind: AuthorizedRootKind::Test,
                    source_hash: snapshot.source_hash(),
                    entry,
                    package_alias: None,
                }],
                package_instances: std::collections::BTreeMap::new(),
                alias_edges: std::collections::BTreeMap::new(),
                limits: policy.limits,
            },
            snapshots: vec![snapshot],
            expected_policy: policy,
        }
    }

    fn trusted_policy() -> EvaluationPolicyReceipt {
        EvaluationPolicyReceipt {
            policy: stable_id("latticeaxiom:nickel-evaluation-policy/developer@1"),
            target: target("x86_64-pc-windows-msvc"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::hard(stable_id(R0_SUPERVISOR_DEADLINE_BACKEND)),
            memory: EnforcementCapability::unsupported(),
            recursion: EnforcementCapability::unsupported(),
        }
    }

    fn production_policy() -> EvaluationPolicyReceipt {
        EvaluationPolicyReceipt {
            policy: stable_id(R0_NICKEL_EVALUATION_POLICY),
            target: target("x86_64-pc-windows-msvc"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::hard(stable_id(R0_SUPERVISOR_DEADLINE_BACKEND)),
            memory: EnforcementCapability::hard(stable_id(R0_WINDOWS_MEMORY_BACKEND)),
            recursion: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-recursion-backend/nickel-vm-frames@1",
            )),
        }
    }

    fn fixture_provenance(source: &[u8]) -> SourceProvenance {
        SourceProvenance::new(
            SourceId::from_str("latticeaxiom:source/worker-fixture")
                .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}")),
            "entry.ncl",
            CanonicalHash::digest(source),
            None,
            Vec::new(),
        )
        .unwrap_or_else(|error| panic!("fixture provenance is invalid: {error}"))
    }

    fn stable_id(value: &str) -> StableId {
        StableId::from_str(value)
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` is invalid: {error}"))
    }

    fn target(value: &str) -> TargetTriple {
        TargetTriple::from_str(value)
            .unwrap_or_else(|error| panic!("fixture target `{value}` is invalid: {error}"))
    }

    struct FixtureRequest {
        directory: TestDirectory,
        request: WorkerRequest,
    }

    impl FixtureRequest {
        fn rescan(&self) -> WorkerRequest {
            request_from_directory(&self.directory, self.request.authoring_target.clone())
        }
    }

    struct TestDirectory {
        path: PathBuf,
    }

    impl TestDirectory {
        fn new() -> Self {
            static NEXT_ID: AtomicU64 = AtomicU64::new(0);
            let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join(format!("latticeaxiom-worker-{}-{id}", std::process::id()));
            fs::create_dir(&path)
                .unwrap_or_else(|error| panic!("fixture directory creation failed: {error}"));
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            if let Err(error) = fs::remove_dir_all(&self.path) {
                panic!("fixture directory cleanup failed: {error}");
            }
        }
    }
}
