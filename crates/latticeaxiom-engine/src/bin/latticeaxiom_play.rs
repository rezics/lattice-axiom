//! Non-Bevy product supervisor entry for `task play`.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let workspace = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "workspace directory is unavailable: {error}"
            );
            return ExitCode::FAILURE;
        }
    };
    match latticeaxiom_engine::run_product_supervisor_from_workspace(&workspace) {
        Ok(report) => {
            let _ = writeln!(
                io::stderr().lock(),
                "supervisor outcome: {:?}",
                report.outcome()
            );
            ExitCode::SUCCESS
        }
        Err(error) => {
            let _ = writeln!(io::stderr().lock(), "{error}");
            ExitCode::FAILURE
        }
    }
}
