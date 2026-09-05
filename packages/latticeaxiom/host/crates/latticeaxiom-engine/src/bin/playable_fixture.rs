//! Non-production local playable fixture.
//!
//! This extra binary keeps the trusted Terrenia seed slice reachable for
//! development. It is not the default engine client and does not boot from a
//! reopened product lock.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match latticeaxiom_engine::run_playable_client() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let mut stderr = io::stderr().lock();
            let _ = writeln!(
                stderr,
                "event=playable_fixture_failed component=engine error={error}"
            );
            ExitCode::FAILURE
        }
    }
}
