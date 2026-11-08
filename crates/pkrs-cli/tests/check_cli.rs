//! `pkrs check` end to end (§2.3, §8.2): every fixture project and template passes, and the
//! broken fixture projects report every problem in one run (golden `insta` snapshots).
//!
//! Snapshots are taken with `NO_COLOR=1`; paths in the output are project-relative and
//! sources are read with `\r\n` normalised, so the text is the same on every OS.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Runs `pkrs check <args> --project <dir>`: (exit code, stdout, stderr), newlines normalised.
fn check(dir: &Path, args: &[&str]) -> (i32, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_pkrs"))
        .arg("check")
        .args(args)
        .arg("--project")
        .arg(dir)
        .env("NO_COLOR", "1")
        .env("RUST_LOG", "off")
        .output()
        .unwrap();
    let norm = |b: &[u8]| String::from_utf8_lossy(b).replace("\r\n", "\n");
    (
        out.status.code().unwrap_or(-1),
        norm(&out.stdout),
        norm(&out.stderr),
    )
}

/// The output as one snapshot: exit code, stdout, stderr.
fn transcript(dir: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = check(dir, args);
    format!("exit code: {code}\n--- stdout\n{stdout}--- stderr\n{stderr}")
}

#[test]
fn every_fixture_project_passes() {
    for name in [
        "battle-town",
        "doubles-arena",
        "grass-route",
        "minimal",
        "resonance-town",
        "scenario-town",
        "story-only",
        "training-grounds",
    ] {
        let dir = root().join("tests/fixtures/projects").join(name);
        for args in [&[][..], &["--plugins"][..]] {
            let (code, stdout, stderr) = check(&dir, args);
            assert_eq!(code, 0, "{name} {args:?}:\n{stdout}{stderr}");
            assert!(stdout.ends_with("check passed\n"), "{name}: {stdout}");
        }
    }
}

fn broken(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/check")
        .join(name)
}

#[test]
fn broken_content_reports_every_problem_in_one_run() {
    insta::assert_snapshot!(transcript(&broken("broken-content"), &[]));
}

#[test]
fn broken_plugins_report_the_plugin_graph() {
    insta::assert_snapshot!(transcript(&broken("broken-plugins"), &[]));
}

#[test]
fn broken_manifest_still_checks_the_rest() {
    insta::assert_snapshot!(transcript(&broken("broken-manifest"), &[]));
}

#[test]
fn warnings_pass_unless_denied() {
    let dir = broken("warnings-only");
    let (code, stdout, _) = check(&dir, &[]);
    assert_eq!(code, 0);
    assert!(stdout.ends_with("check passed\n"));
    insta::assert_snapshot!(transcript(&dir, &["--deny-warnings"]));
}

#[test]
fn a_missing_project_cannot_run() {
    let (code, _, stderr) = check(&broken("does-not-exist"), &[]);
    assert_eq!(code, 2, "{stderr}");
}
