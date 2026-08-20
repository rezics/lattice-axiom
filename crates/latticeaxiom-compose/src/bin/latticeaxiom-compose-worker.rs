//! Internal single-request Nickel evaluator worker.

use std::io::{self, Read, Write};
use std::process::ExitCode;

use latticeaxiom_compose::{
    WORKER_PROTOCOL_LENGTH_PREFIX_BYTES, WORKER_PROTOCOL_MAX_PAYLOAD_BYTES, decode_worker_request,
    encode_worker_response, handle_worker_request,
};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let maximum_frame_bytes = u64::from(WORKER_PROTOCOL_MAX_PAYLOAD_BYTES)
        .saturating_add(u64::try_from(WORKER_PROTOCOL_LENGTH_PREFIX_BYTES).unwrap_or(u64::MAX))
        .saturating_add(1);
    let mut frame = Vec::new();
    io::stdin()
        .lock()
        .take(maximum_frame_bytes)
        .read_to_end(&mut frame)
        .map_err(|error| format!("failed to read worker request: {error}"))?;
    let request = decode_worker_request(&frame)
        .map_err(|error| format!("worker request was rejected: {error}"))?;
    let response = handle_worker_request(request)
        .map_err(|error| format!("worker response construction failed: {error}"))?;
    let response_frame = encode_worker_response(&response)
        .map_err(|error| format!("worker response encoding failed: {error}"))?;
    let mut stdout = io::stdout().lock();
    stdout
        .write_all(&response_frame)
        .and_then(|()| stdout.flush())
        .map_err(|error| format!("failed to write worker response: {error}"))
}
