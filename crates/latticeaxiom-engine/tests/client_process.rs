//! GPU-free selection of shell vs game process from lock graph roots.
//!
//! Compiles with `--no-default-features`; it does not construct a Bevy window
//! or GPU device.
#![allow(clippy::expect_used)]

use latticeaxiom_engine::ProductionMemoryStart;

#[test]
fn front_end_root_without_terrenia_selects_shell_process() {
    assert!(ProductionMemoryStart::lock_roots_select_shell([
        "@latticeaxiom/front-end"
    ]));
}

#[test]
fn terrenia_root_selects_game_process() {
    assert!(!ProductionMemoryStart::lock_roots_select_shell([
        "terrenia"
    ]));
    assert!(
        !ProductionMemoryStart::lock_roots_select_shell(["@latticeaxiom/front-end", "terrenia"]),
        "profiles/dev.toml client-world keeps the production game host"
    );
}

#[test]
fn unrelated_or_empty_roots_do_not_select_shell() {
    assert!(!ProductionMemoryStart::lock_roots_select_shell([
        "@latticeaxiom/settings"
    ]));
    assert!(!ProductionMemoryStart::lock_roots_select_shell(
        None::<&str>
    ));
}
