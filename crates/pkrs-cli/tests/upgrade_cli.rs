//! CLI test (§2.5): `pkrs upgrade` on an old-version fixture, then `pkrs check`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "pkrs-upgrade-cli-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pkrs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pkrs"))
}

fn copy_tree(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &to);
        } else {
            std::fs::copy(entry.path(), &to).unwrap();
        }
    }
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/upgrade/old-draft")
}

#[test]
fn upgrade_is_idempotent_and_then_check_passes() {
    let root = temp_dir("old");
    let project = root.join("old-tale");
    copy_tree(&fixture(), &project);
    let script_before = std::fs::read(project.join("scripts/main.lua")).unwrap();

    let before = pkrs()
        .args(["check", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    assert!(
        !before.status.success(),
        "the draft manifest is not valid yet:\n{}",
        String::from_utf8_lossy(&before.stdout)
    );

    let upgraded = pkrs()
        .args(["upgrade", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&upgraded.stdout);
    assert!(
        upgraded.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&upgraded.stderr)
    );
    for needle in [
        "window-to-display",
        "start-pos-to-position",
        "engine-api-level-to-api",
        "audio.jingle",
        "map.register_terrain",
        "wrote pkrs.toml",
    ] {
        assert!(stdout.contains(needle), "missing {needle} in:\n{stdout}");
    }

    let manifest = std::fs::read_to_string(project.join("pkrs.toml")).unwrap();
    assert!(manifest.contains("[display]"), "{manifest}");
    assert!(manifest.contains("position = [2, 3]"), "{manifest}");
    assert!(manifest.contains("api = 1"), "{manifest}");
    assert!(manifest.contains("version = \"0.1.0\""), "{manifest}");
    assert!(!manifest.contains("api_level"), "{manifest}");
    assert!(!manifest.contains("[window]"), "{manifest}");
    assert!(
        !manifest
            .lines()
            .any(|line| line.trim_start().starts_with("pos ")),
        "{manifest}"
    );
    assert_eq!(
        std::fs::read(project.join("scripts/main.lua")).unwrap(),
        script_before,
        "scripts are reported, not rewritten"
    );

    let checked = pkrs()
        .args(["check", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    assert!(
        checked.status.success(),
        "{}",
        String::from_utf8_lossy(&checked.stderr)
    );
    let check_out = String::from_utf8_lossy(&checked.stdout);
    let check_err = String::from_utf8_lossy(&checked.stderr);
    assert!(check_out.contains("check passed"), "{check_out}");
    assert!(
        check_err.contains("deprecated"),
        "check warns, and still passes:\n{check_err}"
    );

    let pinned = std::fs::read(project.join("pkrs.toml")).unwrap();
    let again = pkrs()
        .args(["upgrade", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    assert!(
        again.status.success(),
        "{}",
        String::from_utf8_lossy(&again.stderr)
    );
    let again_out = String::from_utf8_lossy(&again.stdout);
    assert!(again_out.contains("pkrs.toml unchanged"), "{again_out}");
    assert!(again_out.contains("(none)"), "{again_out}");
    assert_eq!(std::fs::read(project.join("pkrs.toml")).unwrap(), pinned);

    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn refuses_a_downgrade_and_a_partial_version() {
    let root = temp_dir("down");
    let project = root.join("game");
    std::fs::create_dir_all(&project).unwrap();
    let manifest = "\
[game]\nname = \"T\"\nid = \"com.example.t\"\n\
[engine]\nversion = \"1.0.0\"\napi = 1\n\
[start]\nmap = \"start\"\nposition = [0, 0]\n";
    std::fs::write(project.join("pkrs.toml"), manifest).unwrap();

    let down = pkrs()
        .args(["upgrade", "--to", "0.0.0", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    assert_eq!(
        down.status.code(),
        Some(2),
        "{}",
        String::from_utf8_lossy(&down.stderr)
    );
    assert!(String::from_utf8_lossy(&down.stderr).contains("downgrade"));
    assert_eq!(
        std::fs::read_to_string(project.join("pkrs.toml")).unwrap(),
        manifest
    );

    let partial = pkrs()
        .args(["upgrade", "--to", "0.3", "--project"])
        .arg(&project)
        .output()
        .unwrap();
    assert_eq!(partial.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&partial.stderr).contains("not a full version"));
    assert_eq!(
        std::fs::read_to_string(project.join("pkrs.toml")).unwrap(),
        manifest
    );

    std::fs::remove_dir_all(&root).unwrap();
}
