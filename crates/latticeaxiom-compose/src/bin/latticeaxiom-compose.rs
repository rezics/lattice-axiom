//! Public controller CLI for one typed composition evaluation.

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use latticeaxiom_compose::{
    ControlledEvaluation, ControlledEvaluationFailure, WORKER_PROTOCOL_MAX_PAYLOAD_BYTES,
    WorkerCommand, WorkerRequest, WorkerResponse, decode_worker_request_payload,
    evaluate_controlled_request,
};
use latticeaxiom_core::canonical_json_bytes;

const WORKER_STDERR_BYTES: usize = 1024 * 1024;

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(evaluation) => render_response(&evaluation),
        Err(RunError::Controlled(failure)) => render_controller_failure(&failure),
        Err(RunError::Setup(error)) => {
            let _ = writeln!(
                io::stderr().lock(),
                "[{code}] {details}",
                code = error.code,
                details = error.details
            );
            ExitCode::from(2)
        }
    }
}

fn run(mut arguments: impl Iterator<Item = OsString>) -> Result<ControlledEvaluation, RunError> {
    let command = arguments.next().ok_or_else(CliError::usage)?;
    if command != "evaluate" {
        return Err(CliError::usage().into());
    }

    let mut worker = None;
    let mut request = None;
    while let Some(flag) = arguments.next() {
        let value = arguments.next().ok_or_else(CliError::usage)?;
        if flag == "--worker" && worker.is_none() {
            worker = Some(PathBuf::from(value));
        } else if flag == "--request" && request.is_none() {
            request = Some(PathBuf::from(value));
        } else {
            return Err(CliError::usage().into());
        }
    }
    let worker = worker.ok_or_else(CliError::usage)?;
    let request_path = request.ok_or_else(CliError::usage)?;
    let request_bytes = read_request_file(&request_path)?;
    let request = decode_worker_request_payload(&request_bytes)
        .map_err(|error| CliError::protocol(&error))?;
    evaluate(worker, &request)
}

fn evaluate(worker: PathBuf, request: &WorkerRequest) -> Result<ControlledEvaluation, RunError> {
    let command = WorkerCommand::new(worker).map_err(|error| CliError::supervisor(&error))?;
    evaluate_controlled_request(command, request, WORKER_STDERR_BYTES).map_err(RunError::Controlled)
}

fn render_response(evaluation: &ControlledEvaluation) -> ExitCode {
    let mut stdout = io::stdout().lock();
    if stdout
        .write_all(evaluation.canonical_response())
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(2);
    }
    if !evaluation.worker_stderr().is_empty() {
        let mut stderr = io::stderr().lock();
        let _ = stderr.write_all(evaluation.worker_stderr());
        if evaluation.worker_stderr_truncated() {
            let _ = writeln!(
                stderr,
                "\n[compose.worker_protocol] worker stderr truncated"
            );
        }
    }
    match evaluation.response() {
        WorkerResponse::Success { .. } => ExitCode::SUCCESS,
        WorkerResponse::Failure { .. } => ExitCode::FAILURE,
    }
}

fn render_controller_failure(failure: &ControlledEvaluationFailure) -> ExitCode {
    match canonical_json_bytes(failure.response()) {
        Ok(encoded) => {
            let mut stdout = io::stdout().lock();
            if stdout
                .write_all(&encoded)
                .and_then(|()| stdout.write_all(b"\n"))
                .and_then(|()| stdout.flush())
                .is_ok()
            {
                ExitCode::FAILURE
            } else {
                ExitCode::from(2)
            }
        }
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "[compose.worker_protocol] failed to encode controller failure: {error}"
            );
            ExitCode::from(2)
        }
    }
}

fn read_request_file(path: &PathBuf) -> Result<Vec<u8>, CliError> {
    let limit = u64::from(WORKER_PROTOCOL_MAX_PAYLOAD_BYTES);
    let mut file = File::open(path).map_err(|error| CliError {
        code: "compose.worker_protocol",
        details: format!("failed to open request `{}`: {error}", path.display()),
    })?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| CliError {
            code: "compose.worker_protocol",
            details: format!("failed to read request `{}`: {error}", path.display()),
        })?;
    if u64::try_from(bytes.len()).map_or(true, |length| length > limit) {
        return Err(CliError {
            code: "compose.worker_protocol",
            details: format!(
                "request `{}` exceeds the {} byte protocol cap",
                path.display(),
                WORKER_PROTOCOL_MAX_PAYLOAD_BYTES
            ),
        });
    }
    Ok(bytes)
}

#[derive(Debug)]
struct CliError {
    code: &'static str,
    details: String,
}

impl CliError {
    fn usage() -> Self {
        Self {
            code: "compose.worker_protocol",
            details: "usage: latticeaxiom-compose evaluate --worker <absolute-path> --request <request.json>".to_owned(),
        }
    }

    fn protocol(error: &latticeaxiom_compose::WorkerProtocolError) -> Self {
        Self {
            code: error.code(),
            details: error.to_string(),
        }
    }

    fn supervisor(error: &latticeaxiom_compose::SupervisorError) -> Self {
        Self {
            code: error.code(),
            details: error.to_string(),
        }
    }
}

#[derive(Debug)]
enum RunError {
    Setup(CliError),
    Controlled(ControlledEvaluationFailure),
}

impl From<CliError> for RunError {
    fn from(error: CliError) -> Self {
        Self::Setup(error)
    }
}
