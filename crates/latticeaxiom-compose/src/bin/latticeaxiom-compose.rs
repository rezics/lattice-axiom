//! Public controller CLI for evaluation, offline lock, and frozen verify.

use std::io::{self, Write};
use std::process::ExitCode;

use latticeaxiom_compose::{
    CliCommand, CliError, ControlledEvaluation, ControlledEvaluationFailure,
    evaluate_controlled_request, lock_workspace, parse_cli, read_evaluate_request,
    verify_workspace_frozen, worker_command,
};
use latticeaxiom_core::canonical_json_bytes;

const WORKER_STDERR_BYTES: usize = 1024 * 1024;

fn main() -> ExitCode {
    match run(std::env::args_os().skip(1)) {
        Ok(Outcome::Evaluate(evaluation)) => render_evaluation(&evaluation),
        Ok(Outcome::Lock(report)) => render_report(&report),
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

enum Outcome {
    Evaluate(Box<ControlledEvaluation>),
    Lock(latticeaxiom_compose::CliLockReport),
}

fn run(arguments: impl Iterator<Item = std::ffi::OsString>) -> Result<Outcome, RunError> {
    match parse_cli(arguments)? {
        CliCommand::Evaluate { worker, request } => {
            let request_payload = read_evaluate_request(&request)?;
            let command = worker_command(worker)?;
            let evaluation =
                evaluate_controlled_request(command, &request_payload, WORKER_STDERR_BYTES)
                    .map_err(RunError::Controlled)?;
            Ok(Outcome::Evaluate(Box::new(evaluation)))
        }
        CliCommand::Lock(request) => {
            let (_lock, report) = lock_workspace(&request)?;
            Ok(Outcome::Lock(report))
        }
        CliCommand::Verify(request) => {
            let (_lock, report) = verify_workspace_frozen(&request)?;
            Ok(Outcome::Lock(report))
        }
    }
}

fn render_evaluation(evaluation: &ControlledEvaluation) -> ExitCode {
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
        latticeaxiom_compose::WorkerResponse::Success { .. } => ExitCode::SUCCESS,
        latticeaxiom_compose::WorkerResponse::Failure { .. } => ExitCode::FAILURE,
    }
}

fn render_report(report: &latticeaxiom_compose::CliLockReport) -> ExitCode {
    match canonical_json_bytes(report) {
        Ok(encoded) => {
            let mut stdout = io::stdout().lock();
            if stdout
                .write_all(&encoded)
                .and_then(|()| stdout.write_all(b"\n"))
                .and_then(|()| stdout.flush())
                .is_ok()
            {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "[compose.lock] failed to encode report: {error}"
            );
            ExitCode::from(2)
        }
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
