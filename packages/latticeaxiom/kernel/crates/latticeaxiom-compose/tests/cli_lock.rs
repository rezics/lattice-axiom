//! CLI argument parsing and frozen-offline failure for lock/verify.

#![cfg(feature = "nickel-evaluator")]

use std::path::PathBuf;
use std::process::Command;

#[test]
fn verify_without_offline_frozen_flags_fails_closed() {
    let cli_path = PathBuf::from(env!("CARGO_BIN_EXE_latticeaxiom-compose"));
    let output = Command::new(&cli_path)
        .arg("verify")
        .output()
        .unwrap_or_else(|error| panic!("CLI spawn failed: {error}"));
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("compose.lock"),
        "verify usage failure must use compose.lock, got {stderr}"
    );
    assert!(
        stderr.contains("--offline --frozen"),
        "verify must require --offline --frozen, got {stderr}"
    );
}

#[test]
fn frozen_verify_fails_on_missing_lock() {
    let cli_path = PathBuf::from(env!("CARGO_BIN_EXE_latticeaxiom-compose"));
    let directory = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("temporary CLI directory creation failed: {error}"));
    let missing = directory.path().join("missing.lock");
    let output = Command::new(&cli_path)
        .arg("verify")
        .arg("--offline")
        .arg("--frozen")
        .arg("--lock")
        .arg(&missing)
        .output()
        .unwrap_or_else(|error| panic!("CLI spawn failed: {error}"));
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("compose.lock"),
        "missing lock must fail as compose.lock, got {stderr}"
    );
    assert!(
        stderr.contains("missing") || stderr.contains("product lock"),
        "missing lock must name the receipt, got {stderr}"
    );
}
