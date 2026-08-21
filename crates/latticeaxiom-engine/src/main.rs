//! Production client entry point.
//!
//! Ordinary launch reopens `latticeaxiom.lock`, freeze-verifies catalog CAS,
//! and starts the V2/V4 production host through Bevy `DefaultPlugins`. The
//! non-production playable fixture is the extra binary
//! `latticeaxiom-playable-fixture`.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match latticeaxiom_engine::run_client_host_from_lock() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut stderr = io::stderr().lock();
            let _ = writeln!(stderr, "{error}");
            if let Some(hint) = error.recovery_hint() {
                let _ = writeln!(stderr, "{hint}");
            }
            if matches!(
                error,
                latticeaxiom_engine::ProductionClientError::MissingLock { .. }
            ) {
                let _ = writeln!(
                    stderr,
                    "the lock-free development slice is latticeaxiom-playable-fixture"
                );
            }
            ExitCode::FAILURE
        }
    }
}
