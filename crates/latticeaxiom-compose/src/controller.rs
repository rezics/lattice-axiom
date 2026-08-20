//! Complete-request composition controller shared by CLI and embedded callers.

use std::str::FromStr;
use std::time::{Duration, Instant};

use latticeaxiom_core::{TargetTriple, canonical_json_bytes};
use thiserror::Error;

use crate::{
    Diagnostic, DiagnosticCode, DiagnosticSeverity, EnforcementStrength,
    R0_SUPERVISOR_DEADLINE_BACKEND, SupervisorError, SupervisorLimits,
    WORKER_PROTOCOL_MAX_PAYLOAD_BYTES, WorkerCommand, WorkerFailure, WorkerProtocolError,
    WorkerProtocolVersions, WorkerRequest, WorkerResponse, WorkerSupervisor,
    decode_worker_response_payload, encode_worker_request_payload,
};

/// A complete, validated worker result and its canonical presentation bytes.
#[derive(Debug)]
pub struct ControlledEvaluation {
    response: WorkerResponse,
    canonical_response: Vec<u8>,
    worker_stderr: Vec<u8>,
    worker_stderr_truncated: bool,
}

impl ControlledEvaluation {
    /// Returns the typed atomic worker response.
    #[must_use]
    pub const fn response(&self) -> &WorkerResponse {
        &self.response
    }

    /// Returns canonical JSON bytes for the complete response.
    #[must_use]
    pub fn canonical_response(&self) -> &[u8] {
        &self.canonical_response
    }

    /// Returns bounded worker stderr retained for operator diagnostics.
    #[must_use]
    pub fn worker_stderr(&self) -> &[u8] {
        &self.worker_stderr
    }

    /// Reports whether worker stderr exceeded its retention bound.
    #[must_use]
    pub const fn worker_stderr_truncated(&self) -> bool {
        self.worker_stderr_truncated
    }
}

/// Runs one complete controlled evaluation through the shared controller path.
///
/// The monotonic policy deadline starts when this function accepts the typed
/// request. It includes request revalidation and encoding, worker spawn and
/// execution, response decoding, controller-side source-closure recomputation,
/// typed receipt validation, and final canonical response encoding. Immutable
/// source acquisition and lock-scoped alias-edge binding occur before this
/// boundary and are not counted. Production `r0@1` stays fail-closed until a
/// source-table-only loader can consume those bytes and alias edges.
///
/// # Errors
///
/// Returns [`ControlledEvaluationFailure`] with one atomic versioned
/// diagnostic for an invalid request, protocol encoding or validation
/// failure, worker supervisor failure, or expiration of the total
/// complete-request deadline. No partial response is returned.
pub fn evaluate_controlled_request(
    worker: WorkerCommand,
    request: &WorkerRequest,
    worker_stderr_bytes: usize,
) -> Result<ControlledEvaluation, ControlledEvaluationFailure> {
    evaluate_controlled_request_inner(worker, request, worker_stderr_bytes)
        .map_err(ControlledEvaluationFailure::new)
}

fn evaluate_controlled_request_inner(
    worker: WorkerCommand,
    request: &WorkerRequest,
    worker_stderr_bytes: usize,
) -> Result<ControlledEvaluation, ControllerError> {
    let started = Instant::now();
    let configured_deadline = Duration::from_millis(request.expected_policy.limits.wall_clock_ms);
    request
        .validate()
        .map_err(|error| ControllerError::protocol(&error))?;
    validate_installed_controller_capabilities(request)?;
    let payload = encode_worker_request_payload(request)
        .map_err(|error| ControllerError::protocol(&error))?;
    let protocol_limit = usize::try_from(WORKER_PROTOCOL_MAX_PAYLOAD_BYTES).map_err(|_| {
        ControllerError::Protocol {
            details: "worker protocol cap cannot be represented on this platform".to_owned(),
        }
    })?;
    let remaining = remaining_deadline(started, configured_deadline)?;
    let limits = SupervisorLimits::new(
        remaining,
        protocol_limit,
        protocol_limit,
        worker_stderr_bytes,
    )
    .map_err(ControllerError::supervisor)?;
    let execution = WorkerSupervisor::new(worker, limits)
        .execute(&payload, &request.expected_policy, |response_payload| {
            let response = decode_worker_response_payload(response_payload)?;
            response.validate_against_request(request)?;
            Ok::<_, WorkerProtocolError>(response)
        })
        .map_err(ControllerError::supervisor)?;
    let canonical_response =
        canonical_json_bytes(execution.response()).map_err(|error| ControllerError::Protocol {
            details: format!("canonical response encoding failed: {error}"),
        })?;
    ensure_total_deadline(started, configured_deadline)?;
    let worker_stderr = execution.stderr().to_vec();
    let worker_stderr_truncated = execution.stderr_truncated();
    let response = execution.into_response();
    Ok(ControlledEvaluation {
        response,
        canonical_response,
        worker_stderr,
        worker_stderr_truncated,
    })
}

/// One atomic controller-fatal response and its implementation error context.
///
/// The response always contains exactly one error diagnostic and no partial
/// output or receipts. CLI and embedded callers therefore observe the same
/// versioned failure DTO after a typed request has been accepted.
#[derive(Debug, Error)]
#[error("{error}")]
pub struct ControlledEvaluationFailure {
    response: Box<WorkerResponse>,
    error: ControllerError,
}

impl ControlledEvaluationFailure {
    fn new(error: ControllerError) -> Self {
        let diagnostic = Diagnostic {
            code: DiagnosticCode::from_builtin(error.code()),
            severity: DiagnosticSeverity::Error,
            summary: error.to_string(),
            labels: Vec::new(),
            notes: Vec::new(),
        };
        Self {
            response: Box::new(WorkerResponse::Failure {
                versions: WorkerProtocolVersions::current(),
                failure: WorkerFailure {
                    diagnostics: vec![diagnostic],
                    source_closure: None,
                    evaluation: None,
                },
            }),
            error,
        }
    }

    /// Returns the atomic versioned failure response.
    #[must_use]
    pub const fn response(&self) -> &WorkerResponse {
        &self.response
    }

    /// Returns the controller implementation error that selected the stable
    /// diagnostic code and summary.
    #[must_use]
    pub const fn controller_error(&self) -> &ControllerError {
        &self.error
    }
}

/// Returns the concrete containment-host target enforced by this controller.
///
/// # Errors
///
/// Returns [`ControllerError::UnsupportedHostTarget`] when this R0 controller
/// has no frozen mapping for the compilation host.
pub fn controller_host_target() -> Result<TargetTriple, ControllerError> {
    let architecture = std::env::consts::ARCH;
    let value = if cfg!(all(target_os = "windows", target_env = "msvc")) {
        format!("{architecture}-pc-windows-msvc")
    } else if cfg!(all(target_os = "windows", target_env = "gnu")) {
        format!("{architecture}-pc-windows-gnu")
    } else if cfg!(all(target_os = "linux", target_env = "musl")) {
        format!("{architecture}-unknown-linux-musl")
    } else if cfg!(target_os = "linux") {
        format!("{architecture}-unknown-linux-gnu")
    } else if cfg!(target_os = "macos") {
        format!("{architecture}-apple-darwin")
    } else {
        return Err(ControllerError::UnsupportedHostTarget {
            architecture: architecture.to_owned(),
            operating_system: std::env::consts::OS.to_owned(),
        });
    };
    TargetTriple::from_str(&value).map_err(|error| ControllerError::Protocol {
        details: format!("controller host target `{value}` is invalid: {error}"),
    })
}

fn validate_installed_controller_capabilities(
    request: &WorkerRequest,
) -> Result<(), ControllerError> {
    let actual_target = controller_host_target()?;
    if request.worker_target != actual_target || request.expected_policy.target != actual_target {
        return Err(ControllerError::HostTargetMismatch {
            actual: actual_target.to_string(),
            requested: request.worker_target.to_string(),
        });
    }
    let deadline = &request.expected_policy.deadline;
    if deadline.strength != EnforcementStrength::Hard
        || deadline
            .backend
            .as_ref()
            .map(latticeaxiom_core::StableId::as_str)
            != Some(R0_SUPERVISOR_DEADLINE_BACKEND)
    {
        return Err(ControllerError::DeadlineBackendMismatch);
    }
    Ok(())
}

fn remaining_deadline(started: Instant, configured: Duration) -> Result<Duration, ControllerError> {
    configured
        .checked_sub(started.elapsed())
        .filter(|remaining| !remaining.is_zero())
        .ok_or(ControllerError::Timeout {
            deadline_ms: configured.as_millis(),
        })
}

fn ensure_total_deadline(started: Instant, configured: Duration) -> Result<(), ControllerError> {
    if started.elapsed() > configured {
        Err(ControllerError::Timeout {
            deadline_ms: configured.as_millis(),
        })
    } else {
        Ok(())
    }
}

/// Stable complete-controller failure.
#[derive(Debug, Error)]
pub enum ControllerError {
    /// Request or response protocol validation failed.
    #[error("controlled evaluation protocol failed: {details}")]
    Protocol {
        /// Stable validation context.
        details: String,
    },
    /// The worker supervisor failed.
    #[error(transparent)]
    Supervisor(#[from] SupervisorError),
    /// The complete-request deadline expired outside the worker wait loop.
    #[error("controlled evaluation exceeded the {deadline_ms} ms total deadline")]
    Timeout {
        /// Full configured policy deadline.
        deadline_ms: u128,
    },
    /// The request named a containment target other than the running host.
    #[error("worker host target `{requested}` differs from controller host `{actual}`")]
    HostTargetMismatch {
        /// Controller compilation host.
        actual: String,
        /// Requested host identity.
        requested: String,
    },
    /// The controller does not support its current compilation host in R0.
    #[error("unsupported controller host `{architecture}-{operating_system}`")]
    UnsupportedHostTarget {
        /// Host architecture.
        architecture: String,
        /// Host operating system.
        operating_system: String,
    },
    /// The request did not name the hard backend installed by this controller.
    #[error("request does not bind the hard supervisor monotonic deadline backend")]
    DeadlineBackendMismatch,
}

impl ControllerError {
    fn protocol(error: &WorkerProtocolError) -> Self {
        Self::Protocol {
            details: error.to_string(),
        }
    }

    fn supervisor(error: SupervisorError) -> Self {
        Self::Supervisor(error)
    }

    /// Returns the stable ADR 0022 diagnostic code for this failure.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Protocol { .. }
            | Self::HostTargetMismatch { .. }
            | Self::UnsupportedHostTarget { .. }
            | Self::DeadlineBackendMismatch => "compose.worker_protocol",
            Self::Supervisor(error) => error.code(),
            Self::Timeout { .. } => "compose.worker_timeout",
        }
    }
}
