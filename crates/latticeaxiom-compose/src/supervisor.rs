//! Bounded supervision for one process-external evaluator request.
//!
//! The supervisor starts an explicitly named executable, exchanges exactly one
//! length-prefixed request and response, bounds all retained process output,
//! and enforces a monotonic end-to-end deadline. Process separation limits the
//! blast radius of evaluator faults; it is not a hostile-code sandbox. OS
//! memory containment and evaluator recursion metering remain separate,
//! receipt-bearing capabilities.

#[cfg(test)]
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;
use thiserror::Error;

use crate::EvaluationPolicyReceipt;

const FRAME_HEADER_BYTES: usize = size_of::<u32>();
const MAX_FRAME_PAYLOAD_BYTES: usize = 256 * 1024 * 1024;
const PROCESS_POLL_INTERVAL: Duration = Duration::from_millis(1);

/// Versioned hard-deadline backend installed by [`WorkerSupervisor`].
pub const R0_SUPERVISOR_DEADLINE_BACKEND: &str =
    "latticeaxiom:worker-deadline-backend/supervisor-monotonic@1";

/// An explicitly selected worker executable and its literal arguments.
///
/// The executable must be absolute, so process startup never searches an
/// ambient `PATH`. The worker receives an empty environment and starts in the
/// executable's parent directory. Evaluator sources still come exclusively
/// from the framed request rather than the working directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerCommand {
    executable: PathBuf,
    arguments: Vec<OsString>,
    #[cfg(test)]
    test_environment: std::collections::BTreeMap<OsString, OsString>,
}

impl WorkerCommand {
    /// Creates a command for an absolute worker executable path.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::WorkerExecutableNotAbsolute`] when the path
    /// could otherwise trigger ambient executable discovery.
    pub fn new(executable: impl Into<PathBuf>) -> Result<Self, SupervisorError> {
        let executable = executable.into();
        if !executable.is_absolute() {
            return Err(SupervisorError::WorkerExecutableNotAbsolute { executable });
        }
        Ok(Self {
            executable,
            arguments: Vec::new(),
            #[cfg(test)]
            test_environment: std::collections::BTreeMap::new(),
        })
    }

    /// Appends one literal worker argument.
    #[must_use]
    pub fn with_argument(mut self, argument: impl Into<OsString>) -> Self {
        self.arguments.push(argument.into());
        self
    }

    /// Returns the absolute worker executable path.
    #[must_use]
    pub fn executable(&self) -> &Path {
        &self.executable
    }

    /// Returns the literal worker arguments in process order.
    #[must_use]
    pub fn arguments(&self) -> &[OsString] {
        &self.arguments
    }

    #[cfg(test)]
    fn with_test_environment(
        mut self,
        name: impl Into<OsString>,
        value: impl Into<OsString>,
    ) -> Self {
        self.test_environment.insert(name.into(), value.into());
        self
    }
}

/// Inclusive byte bounds and total deadline for one worker exchange.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SupervisorLimits {
    deadline: Duration,
    request_bytes: usize,
    response_bytes: usize,
    stderr_bytes: usize,
}

impl SupervisorLimits {
    /// Creates nonzero worker protocol bounds.
    ///
    /// `request_bytes` and `response_bytes` bound frame payloads. The four-byte
    /// frame header is accounted separately. `stderr_bytes` bounds retained
    /// diagnostic log bytes while the reader continues draining excess output.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError::InvalidLimit`] when any bound is zero or the
    /// response capture size cannot be represented on this host.
    pub fn new(
        deadline: Duration,
        request_bytes: usize,
        response_bytes: usize,
        stderr_bytes: usize,
    ) -> Result<Self, SupervisorError> {
        let limits = Self {
            deadline,
            request_bytes,
            response_bytes,
            stderr_bytes,
        };
        limits.validate()?;
        Ok(limits)
    }

    /// Returns the monotonic deadline for the complete exchange.
    #[must_use]
    pub const fn deadline(self) -> Duration {
        self.deadline
    }

    /// Returns the maximum request payload bytes.
    #[must_use]
    pub const fn request_bytes(self) -> usize {
        self.request_bytes
    }

    /// Returns the maximum response payload bytes.
    #[must_use]
    pub const fn response_bytes(self) -> usize {
        self.response_bytes
    }

    /// Returns the maximum retained stderr bytes.
    #[must_use]
    pub const fn stderr_bytes(self) -> usize {
        self.stderr_bytes
    }

    fn validate(self) -> Result<(), SupervisorError> {
        for (field, value) in [
            ("request_bytes", self.request_bytes),
            ("response_bytes", self.response_bytes),
            ("stderr_bytes", self.stderr_bytes),
        ] {
            if value == 0 {
                return Err(SupervisorError::InvalidLimit { field });
            }
        }
        if self.request_bytes > MAX_FRAME_PAYLOAD_BYTES {
            return Err(SupervisorError::InvalidLimit {
                field: "request_bytes",
            });
        }
        if self.response_bytes > MAX_FRAME_PAYLOAD_BYTES {
            return Err(SupervisorError::InvalidLimit {
                field: "response_bytes",
            });
        }
        if self.deadline.is_zero() {
            return Err(SupervisorError::InvalidLimit { field: "deadline" });
        }
        response_capture_limit(self.response_bytes)?;
        Ok(())
    }
}

/// A decoded response that may carry the worker's evaluation-policy receipt.
///
/// The supervisor compares every reported receipt with the controller's exact
/// expectation before returning the response. An early protocol failure may
/// truthfully have no receipt because containment or evaluation did not start.
/// The response codec remains responsible for enforcing the protocol's atomic
/// success/failure invariants and for requiring a receipt on success.
pub trait SupervisedWorkerResponse {
    /// Returns the policy, target, limits, and enforcement capabilities claimed
    /// by the worker, if evaluation reached a receipt-bearing stage.
    fn evaluation_policy_receipt(&self) -> Option<&EvaluationPolicyReceipt>;
}

/// A successfully decoded worker exchange with every reported receipt validated.
#[derive(Debug)]
pub struct WorkerExecution<R> {
    response: R,
    stderr: Vec<u8>,
    stderr_truncated: bool,
}

impl<R> WorkerExecution<R> {
    /// Returns the validated response.
    #[must_use]
    pub const fn response(&self) -> &R {
        &self.response
    }

    /// Returns retained stderr bytes in emission order.
    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }

    /// Reports whether additional stderr bytes were drained but not retained.
    #[must_use]
    pub const fn stderr_truncated(&self) -> bool {
        self.stderr_truncated
    }

    /// Consumes the execution result and returns the validated response.
    #[must_use]
    pub fn into_response(self) -> R {
        self.response
    }
}

/// Controller for one bounded worker process at a time.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerSupervisor {
    command: WorkerCommand,
    limits: SupervisorLimits,
}

impl WorkerSupervisor {
    /// Creates a supervisor from an explicit command and validated bounds.
    #[must_use]
    pub const fn new(command: WorkerCommand, limits: SupervisorLimits) -> Self {
        Self { command, limits }
    }

    /// Runs one request and decodes its response with a caller-supplied codec.
    ///
    /// `request_payload` is the already validated protocol DTO encoding, without
    /// a frame header. `decode` receives the single bounded response payload.
    /// The supervisor rejects every reported evaluation-policy receipt that
    /// does not exactly match `expected_policy_receipt`. The codec must reject
    /// any protocol outcome that improperly omits a required receipt.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] for invalid controller inputs, process
    /// startup or control failures, timeout, abnormal worker exit, malformed
    /// framing, response decoding failure, or receipt mismatch.
    // Keeping process setup, monitoring, teardown, and receipt validation in
    // one linear scope makes ownership and kill/reap behavior auditable.
    #[allow(clippy::too_many_lines)]
    pub fn execute<R, D, E>(
        &self,
        request_payload: &[u8],
        expected_policy_receipt: &EvaluationPolicyReceipt,
        decode: D,
    ) -> Result<WorkerExecution<R>, SupervisorError>
    where
        R: SupervisedWorkerResponse,
        D: FnOnce(&[u8]) -> Result<R, E>,
        E: fmt::Display,
    {
        let started = Instant::now();
        let deadline = started
            .checked_add(self.limits.deadline)
            .ok_or(SupervisorError::InvalidLimit { field: "deadline" })?;
        if request_payload.len() > self.limits.request_bytes {
            return Err(SupervisorError::RequestTooLarge {
                actual: request_payload.len(),
                limit: self.limits.request_bytes,
            });
        }
        expected_policy_receipt.validate().map_err(|error| {
            SupervisorError::InvalidExpectedReceipt {
                details: error.to_string(),
            }
        })?;

        let request = request_payload.to_vec();
        let mut command = Command::new(&self.command.executable);
        command
            .args(&self.command.arguments)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(parent) = self.command.executable.parent() {
            command.current_dir(parent);
        }
        #[cfg(test)]
        command.envs(&self.command.test_environment);

        let mut child = command
            .spawn()
            .map_err(|error| SupervisorError::SpawnWorker {
                executable: self.command.executable.clone(),
                kind: error.kind(),
            })?;
        let stdin = take_pipe(child.stdin.take(), &mut child, "stdin")?;
        let stdout = take_pipe(child.stdout.take(), &mut child, "stdout")?;
        let stderr = take_pipe(child.stderr.take(), &mut child, "stderr")?;

        let writer = spawn_io_thread("latticeaxiom-worker-stdin", move || {
            write_frame(stdin, &request)
        })
        .map_err(|error| {
            abort_worker(&mut child);
            SupervisorError::StartIoThread {
                stream: "stdin",
                kind: error.kind(),
            }
        })?;
        let response_capture = response_capture_limit(self.limits.response_bytes)?;
        let stdout_reader = match spawn_io_thread("latticeaxiom-worker-stdout", move || {
            read_bounded(stdout, response_capture)
        }) {
            Ok(stdout_reader) => stdout_reader,
            Err(error) => {
                abort_worker(&mut child);
                discard_join(writer);
                return Err(SupervisorError::StartIoThread {
                    stream: "stdout",
                    kind: error.kind(),
                });
            }
        };
        let stderr_capture = self.limits.stderr_bytes;
        let stderr_reader = match spawn_io_thread("latticeaxiom-worker-stderr", move || {
            read_bounded(stderr, stderr_capture)
        }) {
            Ok(stderr_reader) => stderr_reader,
            Err(error) => {
                abort_worker(&mut child);
                discard_join(writer);
                discard_join(stdout_reader);
                return Err(SupervisorError::StartIoThread {
                    stream: "stderr",
                    kind: error.kind(),
                });
            }
        };

        let status = match wait_for_process_and_pipes(
            &mut child,
            &writer,
            &stdout_reader,
            &stderr_reader,
            deadline,
            self.limits.deadline,
        ) {
            Ok(status) => status,
            Err(error) => {
                abort_worker_and_discard_io_threads(
                    &mut child,
                    writer,
                    stdout_reader,
                    stderr_reader,
                );
                return Err(error);
            }
        };
        let write_result = join_io_thread(writer, "stdin");
        let stdout_result = join_io_thread(stdout_reader, "stdout");
        let stderr_result = join_io_thread(stderr_reader, "stderr");
        let write_result = match write_result {
            Ok(result) => result,
            Err(error) => {
                abort_worker(&mut child);
                return Err(error);
            }
        };
        let stdout_result = match stdout_result {
            Ok(result) => result,
            Err(error) => {
                abort_worker(&mut child);
                return Err(error);
            }
        };
        let stderr_result = match stderr_result {
            Ok(result) => result,
            Err(error) => {
                abort_worker(&mut child);
                return Err(error);
            }
        };
        let stderr = match stderr_result {
            Ok(stderr) => stderr,
            Err(error) => {
                abort_worker(&mut child);
                return Err(SupervisorError::PipeIo {
                    stream: "stderr",
                    kind: error.kind(),
                });
            }
        };

        if !status.success() {
            return Err(SupervisorError::WorkerExited {
                status_code: status.code(),
                stderr: String::from_utf8_lossy(&stderr.bytes).into_owned(),
                stderr_truncated: stderr.exceeded,
            });
        }
        if let Err(error) = write_result {
            abort_worker(&mut child);
            return Err(SupervisorError::PipeIo {
                stream: "stdin",
                kind: error.kind(),
            });
        }
        let stdout = match stdout_result {
            Ok(stdout) => stdout,
            Err(error) => {
                abort_worker(&mut child);
                return Err(SupervisorError::PipeIo {
                    stream: "stdout",
                    kind: error.kind(),
                });
            }
        };
        ensure_before_deadline(deadline, self.limits.deadline)?;

        let payload =
            parse_single_frame(&stdout.bytes, stdout.exceeded, self.limits.response_bytes);
        ensure_before_deadline(deadline, self.limits.deadline)?;
        let payload = payload?;
        let response = decode(payload);
        ensure_before_deadline(deadline, self.limits.deadline)?;
        let response = response.map_err(|error| SupervisorError::DecodeResponse {
            details: error.to_string(),
        })?;
        let receipt_validation = response
            .evaluation_policy_receipt()
            .map(|receipt| receipt.validate_against_expected(expected_policy_receipt));
        ensure_before_deadline(deadline, self.limits.deadline)?;
        if let Some(validation) = receipt_validation {
            validation.map_err(|error| SupervisorError::ResponseReceiptMismatch {
                details: error.to_string(),
            })?;
        }

        Ok(WorkerExecution {
            response,
            stderr: stderr.bytes,
            stderr_truncated: stderr.exceeded,
        })
    }

    /// Runs one request whose response payload is strict JSON.
    ///
    /// This is a convenience for the current serde protocol. The same process,
    /// frame, deadline, and reported-receipt checks as [`Self::execute`] apply.
    ///
    /// # Errors
    ///
    /// Returns [`SupervisorError`] for all errors documented by
    /// [`Self::execute`], including JSON decoding failure.
    pub fn execute_json<R>(
        &self,
        request_payload: &[u8],
        expected_policy_receipt: &EvaluationPolicyReceipt,
    ) -> Result<WorkerExecution<R>, SupervisorError>
    where
        R: DeserializeOwned + SupervisedWorkerResponse,
    {
        self.execute(request_payload, expected_policy_receipt, |payload| {
            serde_json::from_slice(payload)
        })
    }
}

#[derive(Debug)]
struct BoundedRead {
    bytes: Vec<u8>,
    exceeded: bool,
}

fn response_capture_limit(response_bytes: usize) -> Result<usize, SupervisorError> {
    response_bytes
        .checked_add(FRAME_HEADER_BYTES)
        .and_then(|value| value.checked_add(1))
        .ok_or(SupervisorError::InvalidLimit {
            field: "response_bytes",
        })
}

fn take_pipe<T>(
    pipe: Option<T>,
    child: &mut Child,
    stream: &'static str,
) -> Result<T, SupervisorError> {
    let Some(pipe) = pipe else {
        abort_worker(child);
        return Err(SupervisorError::MissingPipe { stream });
    };
    Ok(pipe)
}

fn spawn_io_thread<T, F>(name: &str, operation: F) -> io::Result<JoinHandle<T>>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    thread::Builder::new()
        .name(name.to_owned())
        .spawn(operation)
}

fn write_frame(mut writer: impl Write, payload: &[u8]) -> io::Result<()> {
    let payload_length = u32::try_from(payload.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "request frame length exceeds u32",
        )
    })?;
    writer.write_all(&payload_length.to_be_bytes())?;
    writer.write_all(payload)?;
    writer.flush()
}

fn read_bounded(mut reader: impl Read, retained_limit: usize) -> io::Result<BoundedRead> {
    let mut bytes = Vec::new();
    let mut exceeded = false;
    let mut chunk = [0_u8; 8 * 1024];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        let retained = retained_limit.saturating_sub(bytes.len()).min(read);
        bytes.extend_from_slice(&chunk[..retained]);
        exceeded |= retained < read;
    }
    Ok(BoundedRead { bytes, exceeded })
}

fn wait_for_process_and_pipes<W, O, E>(
    child: &mut Child,
    writer: &JoinHandle<W>,
    stdout: &JoinHandle<O>,
    stderr: &JoinHandle<E>,
    deadline: Instant,
    configured_deadline: Duration,
) -> Result<ExitStatus, SupervisorError> {
    let mut status = None;
    loop {
        if status.is_none() {
            status = child
                .try_wait()
                .map_err(|error| SupervisorError::ProcessControl {
                    operation: "poll worker",
                    kind: error.kind(),
                })?;
        }
        if let Some(status) = status
            && writer.is_finished()
            && stdout.is_finished()
            && stderr.is_finished()
        {
            return Ok(status);
        }

        let now = Instant::now();
        if now > deadline {
            if status.is_none() {
                kill_and_reap(child)?;
            }
            return Err(SupervisorError::Timeout {
                deadline_ms: configured_deadline.as_millis(),
            });
        }
        thread::sleep(PROCESS_POLL_INTERVAL.min(deadline.saturating_duration_since(now)));
    }
}

fn kill_and_reap(child: &mut Child) -> Result<(), SupervisorError> {
    match child.kill() {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => {}
        Err(error) => {
            return Err(SupervisorError::ProcessControl {
                operation: "kill timed-out worker",
                kind: error.kind(),
            });
        }
    }
    child
        .wait()
        .map(|_| ())
        .map_err(|error| SupervisorError::ProcessControl {
            operation: "reap timed-out worker",
            kind: error.kind(),
        })
}

fn abort_worker(child: &mut Child) {
    let _kill_result = child.kill();
    let _wait_result = child.wait();
}

fn discard_join<T>(handle: JoinHandle<T>) {
    let _join_result = handle.join();
}

fn abort_worker_and_discard_io_threads<W, O, E>(
    child: &mut Child,
    writer: JoinHandle<W>,
    stdout: JoinHandle<O>,
    stderr: JoinHandle<E>,
) {
    abort_worker(child);
    discard_join(writer);
    discard_join(stdout);
    discard_join(stderr);
}

fn join_io_thread<T>(handle: JoinHandle<T>, stream: &'static str) -> Result<T, SupervisorError> {
    handle
        .join()
        .map_err(|_| SupervisorError::IoThreadPanicked { stream })
}

fn ensure_before_deadline(
    deadline: Instant,
    configured_deadline: Duration,
) -> Result<(), SupervisorError> {
    if Instant::now() > deadline {
        Err(SupervisorError::Timeout {
            deadline_ms: configured_deadline.as_millis(),
        })
    } else {
        Ok(())
    }
}

fn parse_single_frame(
    bytes: &[u8],
    capture_exceeded: bool,
    payload_limit: usize,
) -> Result<&[u8], SupervisorError> {
    if bytes.len() < FRAME_HEADER_BYTES {
        return Err(SupervisorError::UnexpectedEof {
            expected_at_least: FRAME_HEADER_BYTES,
            actual: bytes.len(),
        });
    }
    let mut header = [0_u8; FRAME_HEADER_BYTES];
    header.copy_from_slice(&bytes[..FRAME_HEADER_BYTES]);
    let declared = u32::from_be_bytes(header);
    let declared_bytes =
        usize::try_from(declared).map_err(|_| SupervisorError::ResponseTooLarge {
            declared,
            limit: payload_limit,
        })?;
    if declared_bytes > payload_limit {
        return Err(SupervisorError::ResponseTooLarge {
            declared,
            limit: payload_limit,
        });
    }
    let expected = FRAME_HEADER_BYTES.checked_add(declared_bytes).ok_or(
        SupervisorError::ResponseTooLarge {
            declared: u32::MAX,
            limit: payload_limit,
        },
    )?;
    if bytes.len() < expected {
        return Err(SupervisorError::UnexpectedEof {
            expected_at_least: expected,
            actual: bytes.len(),
        });
    }
    if bytes.len() > expected || capture_exceeded {
        return Err(SupervisorError::TrailingStdout {
            trailing_at_least: bytes.len().saturating_sub(expected).max(1),
        });
    }
    Ok(&bytes[FRAME_HEADER_BYTES..expected])
}

/// A stable supervisor failure class.
#[derive(Debug, Error)]
pub enum SupervisorError {
    /// The executable path could trigger ambient `PATH` lookup.
    #[error("worker executable path is not absolute: {executable}", executable = .executable.display())]
    WorkerExecutableNotAbsolute {
        /// Rejected worker path.
        executable: PathBuf,
    },
    /// A protocol bound was zero or not representable.
    #[error("invalid worker supervisor limit `{field}`")]
    InvalidLimit {
        /// Invalid limit field.
        field: &'static str,
    },
    /// The request payload exceeded its explicit frame bound.
    #[error("worker request payload has {actual} bytes; limit is {limit}")]
    RequestTooLarge {
        /// Actual request payload bytes.
        actual: usize,
        /// Inclusive configured limit.
        limit: usize,
    },
    /// The controller's expected receipt was itself invalid.
    #[error("invalid expected worker policy receipt: {details}")]
    InvalidExpectedReceipt {
        /// Typed receipt validation message.
        details: String,
    },
    /// Starting the explicitly named worker failed.
    #[error("failed to start worker {executable} ({kind:?})", executable = .executable.display())]
    SpawnWorker {
        /// Explicit executable that failed to start.
        executable: PathBuf,
        /// Portable I/O error classification.
        kind: io::ErrorKind,
    },
    /// A pipe requested from the standard library was unexpectedly absent.
    #[error("worker {stream} pipe was not created")]
    MissingPipe {
        /// Missing standard stream.
        stream: &'static str,
    },
    /// Starting a bounded pipe task failed.
    #[error("failed to start worker {stream} task ({kind:?})")]
    StartIoThread {
        /// Affected standard stream.
        stream: &'static str,
        /// Portable I/O error classification.
        kind: io::ErrorKind,
    },
    /// Polling, terminating, or reaping the worker failed.
    #[error("failed to {operation} ({kind:?})")]
    ProcessControl {
        /// Failed controller operation.
        operation: &'static str,
        /// Portable I/O error classification.
        kind: io::ErrorKind,
    },
    /// The total monotonic deadline elapsed.
    #[error("worker exceeded the {deadline_ms} ms total deadline")]
    Timeout {
        /// Configured deadline in milliseconds for diagnostics.
        deadline_ms: u128,
    },
    /// A bounded pipe task panicked.
    #[error("worker {stream} task panicked")]
    IoThreadPanicked {
        /// Affected standard stream.
        stream: &'static str,
    },
    /// Reading or writing a worker pipe failed.
    #[error("worker {stream} I/O failed ({kind:?})")]
    PipeIo {
        /// Affected standard stream.
        stream: &'static str,
        /// Portable I/O error classification.
        kind: io::ErrorKind,
    },
    /// The worker exited unsuccessfully or crashed.
    #[error(
        "worker exited unsuccessfully (status {status_code:?}, stderr_truncated={stderr_truncated}): {stderr}"
    )]
    WorkerExited {
        /// Numeric exit status, or `None` for a signal/exception termination.
        status_code: Option<i32>,
        /// Bounded lossy stderr rendering.
        stderr: String,
        /// Whether stderr exceeded its retention bound.
        stderr_truncated: bool,
    },
    /// Stdout ended before a complete header or payload was received.
    #[error("worker stdout ended at {actual} bytes; at least {expected_at_least} were required")]
    UnexpectedEof {
        /// Minimum bytes required for the declared frame.
        expected_at_least: usize,
        /// Bytes actually retained before EOF.
        actual: usize,
    },
    /// The worker declared a response larger than the configured bound.
    #[error("worker declared {declared} response bytes; limit is {limit}")]
    ResponseTooLarge {
        /// Declared response payload bytes.
        declared: u32,
        /// Inclusive configured payload limit.
        limit: usize,
    },
    /// Bytes followed the one permitted response frame.
    #[error("worker stdout contained at least {trailing_at_least} trailing bytes")]
    TrailingStdout {
        /// Minimum observed bytes beyond the declared frame.
        trailing_at_least: usize,
    },
    /// The response payload did not decode as the expected DTO.
    #[error("failed to decode worker response: {details}")]
    DecodeResponse {
        /// Decoder diagnostic retained for controller logs.
        details: String,
    },
    /// The worker response receipt differed from controller expectations.
    #[error("worker response policy receipt mismatch: {details}")]
    ResponseReceiptMismatch {
        /// Typed receipt comparison diagnostic.
        details: String,
    },
}

impl SupervisorError {
    /// Returns the stable ADR 0022 diagnostic code for this failure.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Timeout { .. } => "compose.worker_timeout",
            Self::WorkerExecutableNotAbsolute { .. }
            | Self::SpawnWorker { .. }
            | Self::MissingPipe { .. }
            | Self::StartIoThread { .. }
            | Self::ProcessControl { .. }
            | Self::IoThreadPanicked { .. }
            | Self::PipeIo { .. }
            | Self::WorkerExited { .. } => "compose.worker_unavailable",
            Self::InvalidLimit { .. }
            | Self::RequestTooLarge { .. }
            | Self::InvalidExpectedReceipt { .. }
            | Self::UnexpectedEof { .. }
            | Self::ResponseTooLarge { .. }
            | Self::TrailingStdout { .. }
            | Self::DecodeResponse { .. }
            | Self::ResponseReceiptMismatch { .. } => "compose.worker_protocol",
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use latticeaxiom_core::{StableId, TargetTriple};
    use serde::Deserialize;

    use super::*;
    use crate::{
        EnforcementCapability, NickelEvaluationLimits, R0_LINUX_MEMORY_BACKEND,
        R0_NICKEL_EVALUATION_POLICY, R0_WINDOWS_MEMORY_BACKEND,
    };

    const HELPER_MODE: &str = "LATTICEAXIOM_SUPERVISOR_TEST_MODE";

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct TestResponse {
        receipt: EvaluationPolicyReceipt,
        value: String,
    }

    impl SupervisedWorkerResponse for TestResponse {
        fn evaluation_policy_receipt(&self) -> Option<&EvaluationPolicyReceipt> {
            Some(&self.receipt)
        }
    }

    #[test]
    fn frame_parser_rejects_eof_oversize_and_trailing_output() {
        let valid = framed(b"response");
        assert_eq!(
            parse_single_frame(&valid, false, 8).ok(),
            Some(b"response".as_slice())
        );

        assert!(matches!(
            parse_single_frame(&valid[..4], false, 8),
            Err(SupervisorError::UnexpectedEof { .. })
        ));
        assert!(matches!(
            parse_single_frame(&valid[..valid.len() - 1], false, 8),
            Err(SupervisorError::UnexpectedEof { .. })
        ));

        let oversized = framed(b"123456789");
        let error = parse_single_frame(&oversized, false, 8)
            .err()
            .unwrap_or_else(|| panic!("oversized response unexpectedly passed"));
        assert_eq!(error.code(), "compose.worker_protocol");
        assert!(matches!(error, SupervisorError::ResponseTooLarge { .. }));

        let mut trailing = valid;
        trailing.push(b'!');
        assert!(matches!(
            parse_single_frame(&trailing, false, 8),
            Err(SupervisorError::TrailingStdout { .. })
        ));
        assert!(matches!(
            parse_single_frame(&trailing[..trailing.len() - 1], true, 8),
            Err(SupervisorError::TrailingStdout { .. })
        ));
    }

    #[test]
    fn limits_enforce_the_protocol_payload_ceiling() {
        assert!(SupervisorLimits::new(Duration::from_secs(1), 0, 1, 1).is_err());
        assert!(
            SupervisorLimits::new(
                Duration::from_secs(1),
                MAX_FRAME_PAYLOAD_BYTES.saturating_add(1),
                1,
                1,
            )
            .is_err()
        );
        assert!(
            SupervisorLimits::new(
                Duration::from_secs(1),
                1,
                MAX_FRAME_PAYLOAD_BYTES.saturating_add(1),
                1,
            )
            .is_err()
        );
    }

    #[test]
    fn json_response_is_strict_and_receipt_comparison_is_exact() {
        let expected = production_receipt();
        let valid = serde_json::to_vec(&serde_json::json!({
            "receipt": expected,
            "value": "ok"
        }))
        .unwrap_or_else(|error| panic!("response fixture serialization failed: {error}"));
        let decoded: TestResponse = serde_json::from_slice(&valid)
            .unwrap_or_else(|error| panic!("valid response fixture did not decode: {error}"));
        assert_eq!(decoded.value, "ok");
        assert!(
            decoded
                .evaluation_policy_receipt()
                .unwrap_or_else(|| panic!("response fixture omitted its receipt"))
                .validate_against_expected(&production_receipt())
                .is_ok()
        );

        let unknown = serde_json::to_vec(&serde_json::json!({
            "receipt": production_receipt(),
            "value": "ok",
            "ambient": true
        }))
        .unwrap_or_else(|error| panic!("response fixture serialization failed: {error}"));
        assert!(serde_json::from_slice::<TestResponse>(&unknown).is_err());

        let mut changed = production_receipt();
        changed.target = target("x86_64-unknown-linux-gnu");
        changed.memory = EnforcementCapability::hard(stable_id(R0_LINUX_MEMORY_BACKEND));
        assert!(changed.validate().is_ok());
        assert!(
            changed
                .validate_against_expected(&production_receipt())
                .is_err()
        );
    }

    #[test]
    fn missing_explicit_executable_maps_to_worker_unavailable() {
        let executable = std::env::temp_dir()
            .join(format!(
                "latticeaxiom-missing-worker-{}",
                std::process::id()
            ))
            .join("worker-does-not-exist");
        let command = WorkerCommand::new(executable)
            .unwrap_or_else(|error| panic!("absolute worker command was rejected: {error}"));
        let supervisor = WorkerSupervisor::new(command, limits(Duration::from_secs(1)));
        let result: Result<WorkerExecution<TestResponse>, SupervisorError> =
            supervisor.execute_json(b"{}", &production_receipt());
        let error = result
            .err()
            .unwrap_or_else(|| panic!("missing worker unexpectedly started"));
        assert_eq!(error.code(), "compose.worker_unavailable");
        assert!(matches!(error, SupervisorError::SpawnWorker { .. }));
    }

    #[test]
    fn timed_out_worker_is_killed_reaped_and_classified() {
        let supervisor = helper_supervisor("sleep", Duration::from_millis(100));
        let result: Result<WorkerExecution<TestResponse>, SupervisorError> =
            supervisor.execute_json(b"{}", &production_receipt());
        let error = result
            .err()
            .unwrap_or_else(|| panic!("sleeping worker escaped its deadline"));
        assert_eq!(error.code(), "compose.worker_timeout");
        assert!(matches!(error, SupervisorError::Timeout { .. }));
    }

    #[test]
    fn fault_cleanup_kills_reaps_and_joins_every_io_thread() {
        let mut child = spawn_helper_process("cleanup-sleep");
        let joined = Arc::new(AtomicUsize::new(0));
        let writer = thread::spawn(|| {
            panic!("injected worker stdin task panic");
        });
        let stdout_joined = Arc::clone(&joined);
        let stdout_reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            stdout_joined.fetch_add(1, Ordering::SeqCst);
        });
        let stderr_joined = Arc::clone(&joined);
        let stderr_reader = thread::spawn(move || {
            thread::sleep(Duration::from_millis(25));
            stderr_joined.fetch_add(1, Ordering::SeqCst);
        });

        let started = Instant::now();
        abort_worker_and_discard_io_threads(&mut child, writer, stdout_reader, stderr_reader);

        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(joined.load(Ordering::SeqCst), 2);
        assert!(
            matches!(child.try_wait(), Ok(Some(_))),
            "fault cleanup did not reap the worker"
        );
    }

    #[test]
    fn nonzero_worker_exit_is_unavailable_and_stderr_is_bounded() {
        let supervisor = helper_supervisor("exit", Duration::from_secs(2));
        let result: Result<WorkerExecution<TestResponse>, SupervisorError> =
            supervisor.execute_json(b"{}", &production_receipt());
        let error = result
            .err()
            .unwrap_or_else(|| panic!("nonzero worker exit unexpectedly succeeded"));
        assert_eq!(error.code(), "compose.worker_unavailable");
        let SupervisorError::WorkerExited {
            status_code,
            stderr,
            stderr_truncated,
        } = error
        else {
            panic!("nonzero worker returned the wrong error variant");
        };
        assert_eq!(status_code, Some(23));
        assert!(stderr.len() <= 16);
        assert!(stderr_truncated);
    }

    #[test]
    fn worker_helper() {
        let Some(mode) = std::env::var_os(HELPER_MODE) else {
            return;
        };
        if mode == OsStr::new("cleanup-sleep") {
            thread::sleep(Duration::from_secs(30));
        } else if mode == OsStr::new("sleep") {
            thread::sleep(Duration::from_secs(5));
        } else if mode == OsStr::new("exit") {
            let mut stderr = io::stderr().lock();
            let _write_result = stderr.write_all(b"bounded worker failure details");
            let _flush_result = stderr.flush();
            std::process::exit(23);
        }
    }

    fn spawn_helper_process(mode: &str) -> Child {
        let executable = std::env::current_exe()
            .unwrap_or_else(|error| panic!("test executable path is unavailable: {error}"));
        Command::new(executable)
            .arg("--exact")
            .arg("supervisor::tests::worker_helper")
            .arg("--nocapture")
            .env(HELPER_MODE, mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap_or_else(|error| panic!("worker helper process did not start: {error}"))
    }

    fn helper_supervisor(mode: &str, deadline: Duration) -> WorkerSupervisor {
        let executable = std::env::current_exe()
            .unwrap_or_else(|error| panic!("test executable path is unavailable: {error}"));
        let command = WorkerCommand::new(executable)
            .unwrap_or_else(|error| panic!("test executable path was rejected: {error}"))
            .with_argument("--exact")
            .with_argument("supervisor::tests::worker_helper")
            .with_argument("--nocapture")
            .with_test_environment(HELPER_MODE, mode);
        WorkerSupervisor::new(command, limits(deadline))
    }

    fn limits(deadline: Duration) -> SupervisorLimits {
        SupervisorLimits::new(deadline, 1024, 1024, 16)
            .unwrap_or_else(|error| panic!("test supervisor limits are invalid: {error}"))
    }

    fn framed(payload: &[u8]) -> Vec<u8> {
        let length = u32::try_from(payload.len())
            .unwrap_or_else(|error| panic!("fixture frame is too large: {error}"));
        let mut frame = length.to_be_bytes().to_vec();
        frame.extend_from_slice(payload);
        frame
    }

    fn production_receipt() -> EvaluationPolicyReceipt {
        EvaluationPolicyReceipt {
            policy: stable_id(R0_NICKEL_EVALUATION_POLICY),
            target: target("x86_64-pc-windows-msvc"),
            limits: NickelEvaluationLimits::default(),
            deadline: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-deadline-backend/supervisor-monotonic@1",
            )),
            memory: EnforcementCapability::hard(stable_id(R0_WINDOWS_MEMORY_BACKEND)),
            recursion: EnforcementCapability::hard(stable_id(
                "latticeaxiom:worker-recursion-backend/nickel-vm-frames@1",
            )),
        }
    }

    fn target(value: &str) -> TargetTriple {
        value
            .parse()
            .unwrap_or_else(|error| panic!("fixture target `{value}` is invalid: {error}"))
    }

    fn stable_id(value: &str) -> StableId {
        StableId::from_str(value)
            .unwrap_or_else(|error| panic!("fixture stable ID `{value}` is invalid: {error}"))
    }
}
