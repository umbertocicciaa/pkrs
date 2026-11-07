//! Integration (§14.4 smoke test): the real `pkrs-runtime` binary runs headless and exits 0,
//! and reports failures with a non-zero status. The CI smoke job runs the same
//! `--headless --ticks 600 --project … --script tests/smoke.lua` command line.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn runtime() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
}

#[test]
fn headless_600_ticks_exits_zero() {
    let output = runtime()
        .args(["--headless", "--ticks", "600"])
        .env("RUST_LOG", "info")
        .output()
        .expect("spawn pkrs-runtime");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "runtime failed:\n{stderr}");
    assert!(
        stderr.contains("headless run finished"),
        "missing completion log:\n{stderr}"
    );
    assert!(stderr.contains("TickLimit"), "unexpected exit:\n{stderr}");
}

/// The fixture project the smoke scenario runs against.
fn scenario_town() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/projects/scenario-town")
}

#[test]
fn smoke_scenario_exits_zero() {
    let output = runtime()
        .args(["--headless", "--ticks", "600", "--project"])
        .arg(scenario_town())
        .args(["--script", "tests/smoke.lua"])
        .env("RUST_LOG", "warn")
        .output()
        .expect("spawn pkrs-runtime");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "smoke scenario failed:
{stderr}"
    );
    assert!(stderr.contains("PASS tests/smoke.lua"), "{stderr}");
}

#[test]
fn failing_scenario_exits_non_zero_with_location() {
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scenarios/failing.lua");
    let output = runtime()
        .arg("--project")
        .arg(scenario_town())
        .arg("--script")
        .arg(&script)
        .env("RUST_LOG", "warn")
        .output()
        .expect("spawn pkrs-runtime");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !output.status.success(),
        "a failed expectation must exit non-zero"
    );
    assert!(stderr.contains("FAIL "), "{stderr}");
    assert!(
        stderr.contains("failing.lua:3: expect_state failed at tick 2"),
        "{stderr}"
    );
    assert!(stderr.contains("expected: Dialogue"), "{stderr}");
    assert!(stderr.contains("actual: Overworld"), "{stderr}");
}

#[test]
fn missing_scenario_exits_non_zero() {
    let output = runtime()
        .arg("--project")
        .arg(scenario_town())
        .args(["--script", "tests/does_not_exist.lua"])
        .output()
        .expect("spawn pkrs-runtime");
    assert!(!output.status.success());
}

#[test]
fn bad_arguments_exit_non_zero() {
    let output = runtime()
        .args(["--headless", "--definitely-not-a-flag"])
        .output()
        .expect("spawn pkrs-runtime");
    assert!(!output.status.success());
}
