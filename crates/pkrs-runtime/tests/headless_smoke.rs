//! Integration (§14.4 smoke test): the real `pkrs-runtime` binary runs headless and exits 0,
//! and reports failures with a non-zero status.

#![allow(clippy::unwrap_used, clippy::expect_used)]

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

#[test]
fn bad_arguments_exit_non_zero() {
    let output = runtime()
        .args(["--headless", "--definitely-not-a-flag"])
        .output()
        .expect("spawn pkrs-runtime");
    assert!(!output.status.success());
}
