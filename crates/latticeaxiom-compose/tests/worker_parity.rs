//! End-to-end parity across the shared embedded controller and public CLI.

#![cfg(feature = "nickel-evaluator")]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};

use latticeaxiom_compose::{
    AuthoringTarget, AuthorizedRoot, AuthorizedRootKind, EnforcementCapability,
    EvaluationPolicyReceipt, NickelEvaluationLimits, R0_SUPERVISOR_DEADLINE_BACKEND, SourceAddress,
    SourceClosureRequest, SourceRootGrant, SourceScanLimits, WorkerCommand, WorkerExecutionClass,
    WorkerProtocolVersions, WorkerRequest, WorkerResponse, controller_host_target,
    encode_worker_request_payload, evaluate_controlled_request, scan_source_snapshot,
};
use latticeaxiom_core::{SourceId, StableId, canonical_json_bytes};

const TOOL_PACKAGE: &str =
    include_str!("../../../fixtures/composition/controlled/import-free-tool-package.ncl");

#[test]
fn embedded_controller_and_cli_return_identical_typed_output() {
    let fixture = ParityFixture::new();
    let request = fixture.request();
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_latticeaxiom-compose-worker"));
    let cli_path = PathBuf::from(env!("CARGO_BIN_EXE_latticeaxiom-compose"));

    let embedded = evaluate_embedded(&worker_path, &request);
    let cli = evaluate_cli(&cli_path, &worker_path, fixture.request_path(), &request);

    let embedded_bytes = canonical_json_bytes(&embedded)
        .unwrap_or_else(|error| panic!("embedded response encoding failed: {error}"));
    let cli_bytes = canonical_json_bytes(&cli)
        .unwrap_or_else(|error| panic!("CLI response encoding failed: {error}"));
    assert_eq!(cli_bytes, embedded_bytes);
}

#[test]
fn embedded_controller_and_cli_return_identical_atomic_failure() {
    let fixture = ParityFixture::new();
    let mut request = fixture.request();
    request.expected_policy.deadline = EnforcementCapability::unsupported();
    fixture.write_request(&request);
    let worker_path = PathBuf::from(env!("CARGO_BIN_EXE_latticeaxiom-compose-worker"));
    let cli_path = PathBuf::from(env!("CARGO_BIN_EXE_latticeaxiom-compose"));

    let command = WorkerCommand::new(&worker_path)
        .unwrap_or_else(|error| panic!("worker command was invalid: {error}"));
    let embedded_failure = evaluate_controlled_request(command, &request, 1024 * 1024)
        .unwrap_err()
        .response()
        .clone();
    let cli_failure =
        evaluate_cli_failure(&cli_path, &worker_path, fixture.request_path(), &request);

    let embedded_bytes = canonical_json_bytes(&embedded_failure)
        .unwrap_or_else(|error| panic!("embedded failure encoding failed: {error}"));
    let cli_bytes = canonical_json_bytes(&cli_failure)
        .unwrap_or_else(|error| panic!("CLI failure encoding failed: {error}"));
    assert_eq!(cli_bytes, embedded_bytes);
    let WorkerResponse::Failure { failure, .. } = embedded_failure else {
        panic!("controller capability mismatch unexpectedly succeeded");
    };
    assert_eq!(failure.diagnostics.len(), 1);
    assert_eq!(
        failure.diagnostics[0].code.as_str(),
        "compose.worker_protocol"
    );
}

fn evaluate_embedded(worker_path: &Path, request: &WorkerRequest) -> WorkerResponse {
    let command = WorkerCommand::new(worker_path)
        .unwrap_or_else(|error| panic!("worker command was invalid: {error}"));
    evaluate_controlled_request(command, request, 1024 * 1024)
        .unwrap_or_else(|error| panic!("embedded controller failed: {error}"))
        .response()
        .clone()
}

fn evaluate_cli(
    cli_path: &Path,
    worker_path: &Path,
    request_path: &Path,
    request: &WorkerRequest,
) -> WorkerResponse {
    let output = Command::new(cli_path)
        .arg("evaluate")
        .arg("--worker")
        .arg(worker_path)
        .arg("--request")
        .arg(request_path)
        .output()
        .unwrap_or_else(|error| panic!("CLI spawn failed: {error}"));
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let response = serde_json::from_slice::<WorkerResponse>(&output.stdout)
        .unwrap_or_else(|error| panic!("CLI response decode failed: {error}"));
    response
        .validate_against_request(request)
        .unwrap_or_else(|error| panic!("CLI response validation failed: {error}"));
    response
}

fn evaluate_cli_failure(
    cli_path: &Path,
    worker_path: &Path,
    request_path: &Path,
    request: &WorkerRequest,
) -> WorkerResponse {
    let output = Command::new(cli_path)
        .arg("evaluate")
        .arg("--worker")
        .arg(worker_path)
        .arg("--request")
        .arg(request_path)
        .output()
        .unwrap_or_else(|error| panic!("CLI spawn failed: {error}"));
    assert_eq!(output.status.code(), Some(1));
    let response = serde_json::from_slice::<WorkerResponse>(&output.stdout)
        .unwrap_or_else(|error| panic!("CLI failure decode failed: {error}"));
    response
        .validate_against_request(request)
        .unwrap_or_else(|error| panic!("CLI failure validation failed: {error}"));
    response
}

struct ParityFixture {
    directory: PathBuf,
    snapshot: latticeaxiom_compose::SourceSnapshot,
    entry: SourceAddress,
    source_id: SourceId,
    request_path: PathBuf,
}

impl ParityFixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let directory = std::env::temp_dir().join(format!(
            "latticeaxiom-worker-parity-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap_or_else(|error| {
            panic!(
                "failed to create parity fixture {}: {error}",
                directory.display()
            )
        });
        fs::write(directory.join("entry.ncl"), TOOL_PACKAGE)
            .unwrap_or_else(|error| panic!("failed to write parity source: {error}"));
        let source_id = SourceId::from_str("latticeaxiom:source/worker-parity")
            .unwrap_or_else(|error| panic!("fixture source ID is invalid: {error}"));
        let root = AuthorizedRoot::new(source_id.clone(), AuthorizedRootKind::Test, &directory)
            .unwrap_or_else(|error| panic!("fixture root is invalid: {error}"));
        let snapshot = scan_source_snapshot(
            &root,
            SourceScanLimits {
                maximum_files: 4,
                maximum_bytes: 64 * 1024,
            },
        )
        .unwrap_or_else(|error| panic!("fixture scan failed: {error}"));
        let entry = SourceAddress::new(source_id.clone(), "entry.ncl")
            .unwrap_or_else(|error| panic!("fixture entry is invalid: {error}"));
        let request_path = directory.join("request.json");
        let fixture = Self {
            directory,
            snapshot,
            entry,
            source_id,
            request_path,
        };
        let request = fixture.request();
        let bytes = encode_worker_request_payload(&request)
            .unwrap_or_else(|error| panic!("fixture request encoding failed: {error}"));
        fs::write(&fixture.request_path, bytes)
            .unwrap_or_else(|error| panic!("fixture request write failed: {error}"));
        fixture
    }

    fn request(&self) -> WorkerRequest {
        let limits = NickelEvaluationLimits::default();
        let target = controller_host_target()
            .unwrap_or_else(|error| panic!("fixture host target is unsupported: {error}"));
        let expected_policy = EvaluationPolicyReceipt {
            policy: StableId::from_str("latticeaxiom:nickel-evaluation-policy/developer@1")
                .unwrap_or_else(|error| panic!("fixture policy is invalid: {error}")),
            target: target.clone(),
            limits,
            deadline: EnforcementCapability::hard(
                StableId::from_str(R0_SUPERVISOR_DEADLINE_BACKEND)
                    .unwrap_or_else(|error| panic!("deadline backend is invalid: {error}")),
            ),
            memory: EnforcementCapability::unsupported(),
            recursion: EnforcementCapability::unsupported(),
        };
        WorkerRequest {
            versions: WorkerProtocolVersions::current(),
            authoring_target: AuthoringTarget::Package,
            worker_target: target,
            execution_class: WorkerExecutionClass::TrustedFixture,
            source_closure: SourceClosureRequest {
                library_contract_major: WorkerProtocolVersions::current().library_contract_major,
                corpus_major: WorkerProtocolVersions::current().corpus_major,
                entry: self.entry.clone(),
                root_grants: vec![SourceRootGrant {
                    source_id: self.source_id.clone(),
                    root_kind: AuthorizedRootKind::Test,
                    source_hash: self.snapshot.source_hash(),
                    entry: self.entry.clone(),
                    package_alias: None,
                }],
                limits,
            },
            snapshots: vec![self.snapshot.clone()],
            expected_policy,
        }
    }

    fn request_path(&self) -> &Path {
        &self.request_path
    }

    fn write_request(&self, request: &WorkerRequest) {
        let bytes = encode_worker_request_payload(request)
            .unwrap_or_else(|error| panic!("fixture request encoding failed: {error}"));
        fs::write(&self.request_path, bytes)
            .unwrap_or_else(|error| panic!("fixture request write failed: {error}"));
    }
}

impl Drop for ParityFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}
