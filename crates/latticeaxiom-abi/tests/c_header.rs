//! C11 compile smoke for the generated public header.

use std::{path::PathBuf, process::Command};

#[test]
fn generated_c_header_compiles_when_a_c_compiler_is_available() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let include = manifest.join("include");
    let source = manifest.join("tests/fixtures/c_header_smoke.c");

    let clang = Command::new("clang")
        .arg("-std=c11")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Werror")
        .arg("-fsyntax-only")
        .arg(format!("-I{}", include.display()))
        .arg(&source)
        .output();
    if let Ok(output) = clang {
        assert!(
            output.status.success(),
            "clang rejected generated header:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let gcc = Command::new("gcc")
        .arg("-std=c11")
        .arg("-Wall")
        .arg("-Wextra")
        .arg("-Werror")
        .arg("-fsyntax-only")
        .arg(format!("-I{}", include.display()))
        .arg(&source)
        .output();
    if let Ok(output) = gcc {
        assert!(
            output.status.success(),
            "gcc rejected generated header:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let msvc = Command::new("cl")
        .arg("/nologo")
        .arg("/std:c11")
        .arg("/W4")
        .arg("/WX")
        .arg("/Zs")
        .arg(format!("/I{}", include.display()))
        .arg(&source)
        .output();
    if let Ok(output) = msvc {
        assert!(
            output.status.success(),
            "MSVC rejected generated header:\n{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
