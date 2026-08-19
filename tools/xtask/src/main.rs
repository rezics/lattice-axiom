//! Development automation, exposed as `cargo xtask <task>`.
//!
//! Tasks mirror CI so "green locally" means "green in CI":
//!
//! - `ci`: fmt-check, clippy (`-Dwarnings`), tests, headless smoke, cargo-deny
//! - `fmt`: apply formatting
//! - `lint`: clippy with `-Dwarnings`
//! - `test`: workspace tests plus the headless smoke run
//! - `deny`: license/advisory/boundary checks (needs `cargo-deny` installed)
//!
//! Status output goes to stderr; this binary never prints to stdout.

use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let task = args.first().map_or("ci", String::as_str);

    let ok = match task {
        "ci" => run_ci(),
        "fmt" => cargo(&["fmt", "--all"]),
        "lint" => lint(),
        "test" => test(),
        "deny" => deny(true),
        other => {
            eprintln!("unknown task {other:?}");
            eprintln!("usage: cargo xtask [ci|fmt|lint|test|deny]");
            false
        }
    };

    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn run_ci() -> bool {
    cargo(&["fmt", "--all", "--check"]) && lint() && test() && deny(false)
}

fn lint() -> bool {
    cargo(&["clippy", "--workspace", "--all-targets", "--", "-Dwarnings"])
}

fn test() -> bool {
    cargo(&["test", "--workspace"])
        && cargo(&[
            "run",
            "--package",
            "latticeaxiom-demo",
            "--",
            "--headless",
            "3",
        ])
}

/// Runs cargo-deny; when `required` is false a missing installation is
/// reported and skipped, because CI runs cargo-deny through its own action.
fn deny(required: bool) -> bool {
    let installed = Command::new("cargo")
        .args(["deny", "--version"])
        .output()
        .is_ok_and(|output| output.status.success());
    if !installed {
        if required {
            eprintln!(
                "cargo-deny is not installed; install with: cargo install cargo-deny --locked"
            );
            return false;
        }
        eprintln!("skipping cargo-deny (not installed locally; CI still enforces it)");
        return true;
    }
    cargo(&["deny", "check"])
}

fn cargo(args: &[&str]) -> bool {
    eprintln!("xtask> cargo {}", args.join(" "));
    let status = Command::new("cargo").args(args).status();
    match status {
        Ok(status) if status.success() => true,
        Ok(status) => {
            eprintln!("xtask> failed with {status}");
            false
        }
        Err(error) => {
            eprintln!("xtask> failed to launch cargo: {error}");
            false
        }
    }
}
