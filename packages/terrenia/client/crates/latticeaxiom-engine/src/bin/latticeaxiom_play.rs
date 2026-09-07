//! Non-Bevy product supervisor entry for `task play`.

use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let workspace = match std::env::current_dir() {
        Ok(path) => path,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "event=supervisor_boot_failed component=supervisor error=workspace directory is unavailable: {error}"
            );
            return ExitCode::FAILURE;
        }
    };
    match latticeaxiom_engine::run_product_supervisor_from_workspace(&workspace) {
        Ok(report) => {
            if std::env::var_os("LATTICEAXIOM_LIFECYCLE_QA").is_some()
                && workspace.join(".latticeaxiom-qa").is_file()
                && let Ok(bytes) = serde_json::to_vec_pretty(&report)
            {
                let _ = std::fs::write(workspace.join("lifecycle-report.json"), bytes);
            }
            let _ = writeln!(
                io::stderr().lock(),
                "event=supervisor_stopped component=supervisor outcome={:?} failures={:?}",
                report.outcome(),
                report.failures()
            );
            if matches!(
                report.outcome(),
                latticeaxiom_launcher::SupervisorOutcomeV1::ProductExited { .. }
            ) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "event=supervisor_failed component=supervisor error={error}"
            );
            ExitCode::FAILURE
        }
    }
}
