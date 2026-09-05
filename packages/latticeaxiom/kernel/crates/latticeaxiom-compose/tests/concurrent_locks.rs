//! Independent product locks may publish shared catalog objects concurrently.

#![cfg(feature = "nickel-evaluator")]
#![allow(
    clippy::expect_used,
    reason = "isolated integration fixture invariants"
)]

use std::{
    path::Path,
    process::{Command, Output},
    sync::Barrier,
};

const COMPOSER: &str = env!("CARGO_BIN_EXE_latticeaxiom-compose");
const WORKSPACE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../../..");

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn game_shell_and_resources_share_a_cold_catalog_across_processes() {
    let directory = tempfile::tempdir().expect("isolated catalog");
    let catalog = directory.path().join("catalog");
    let profiles = ["dev", "shell", "resources"];
    let barrier = Barrier::new(profiles.len());
    std::thread::scope(|scope| {
        let handles = profiles.map(|profile| {
            let catalog = &catalog;
            let directory = directory.path();
            let barrier = &barrier;
            scope.spawn(move || {
                let bootstrap = format!("profiles/{profile}.toml");
                let lock = directory.join(format!("{profile}.lock"));
                barrier.wait();
                let output = Command::new(COMPOSER)
                    .args(["lock", "--offline", "--workspace", WORKSPACE, "--bootstrap"])
                    .arg(bootstrap)
                    .arg("--catalog")
                    .arg(catalog)
                    .arg("--lock")
                    .arg(&lock)
                    .output()
                    .expect("lock process");
                assert_success(&output);
                lock
            })
        });
        for handle in handles {
            let lock = handle.join().expect("lock worker");
            verify(&catalog, &lock);
        }
    });
}

fn verify(catalog: &Path, lock: &Path) {
    let before = std::fs::read(lock).expect("published lock");
    let output = Command::new(COMPOSER)
        .args(["verify", "--offline", "--frozen", "--workspace", WORKSPACE])
        .arg("--catalog")
        .arg(catalog)
        .arg("--lock")
        .arg(lock)
        .output()
        .expect("frozen verification process");
    assert_success(&output);
    assert_eq!(std::fs::read(lock).expect("verified lock"), before);
}
