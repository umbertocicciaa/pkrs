//! CLI test (§2.3): `pkrs new` scaffolds the one-shot template into a temp dir.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::process::Command;

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pkrs() -> Command {
    Command::new(env!("CARGO_BIN_EXE_pkrs"))
}

#[test]
fn new_scaffolds_a_pinned_one_shot_project() {
    let root = temp_dir("new");
    let dest = root.join("demo");
    let out = pkrs()
        .args(["new", "Demo Quest", "--path"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("com.example.demo-quest"));

    for file in [
        "pkrs.toml",
        ".gitignore",
        "LICENSES.md",
        "README.md",
        "maps/start.tmx",
        "maps/dest.tmx",
        "tilesets/overworld.tsx",
        "tilesets/overworld.png",
        "sprites/player.png",
    ] {
        assert!(dest.join(file).is_file(), "missing {file}");
    }
    assert!(!dest.join("_gitignore").exists());

    let manifest = std::fs::read_to_string(dest.join("pkrs.toml")).unwrap();
    assert!(manifest.contains("name    = \"Demo Quest\""), "{manifest}");
    let pinned = format!("version = \"{}\"", env!("CARGO_PKG_VERSION"));
    assert!(manifest.contains(&pinned), "{manifest}");
    assert!(!manifest.contains("{{"), "every placeholder is rendered");
    let readme = std::fs::read_to_string(dest.join("README.md")).unwrap();
    assert!(readme.starts_with("# Demo Quest"));
    let png = std::fs::read(dest.join("tilesets/overworld.png")).unwrap();
    assert!(
        png.starts_with(b"\x89PNG"),
        "binary files are copied untouched"
    );

    // A second `new` into the same, now non-empty, folder is refused.
    let again = pkrs()
        .args(["new", "demo", "--path"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("not empty"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn new_rejects_bad_names_and_unavailable_templates() {
    let root = temp_dir("new-bad");
    let bad = pkrs()
        .args(["new", "../escape", "--path"])
        .arg(root.join("x"))
        .output()
        .unwrap();
    assert!(!bad.status.success());
    let full = pkrs()
        .args(["new", "big", "--template", "full-game", "--path"])
        .arg(root.join("big"))
        .output()
        .unwrap();
    assert!(!full.status.success());
    assert!(String::from_utf8_lossy(&full.stderr).contains("not available yet"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn run_outside_a_project_explains_itself() {
    let root = temp_dir("run-none");
    let out = pkrs()
        .args(["run", "--project"])
        .arg(&root)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no pkrs.toml"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn check_plugins_prints_the_resolved_hook_table() {
    let root = temp_dir("check");
    let dest = root.join("game");
    let out = pkrs()
        .args(["new", "game", "--path"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(out.status.success());
    let manifest = std::fs::read_to_string(dest.join("pkrs.toml")).unwrap();
    std::fs::write(
        dest.join("pkrs.toml"),
        format!("{manifest}\n[plugins]\nenabled = [\"shield\", \"crit\"]\n"),
    )
    .unwrap();
    for (id, priority) in [("shield", -5), ("crit", 10)] {
        let dir = dest.join("plugins").join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("plugin.toml"),
            format!("[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = 1\n"),
        )
        .unwrap();
        std::fs::write(
            dir.join("init.lua"),
            format!("battle.on('on_damage_taken', function() end, {{ priority = {priority} }})\n"),
        )
        .unwrap();
    }
    let out = pkrs()
        .args(["check", "--plugins", "--project"])
        .arg(&dest)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("plugins (load order): shield, crit\n"),
        "{stdout}"
    );
    assert!(
        stdout.contains("  on_damage_taken\n      -5  shield\n      10  crit\n"),
        "{stdout}"
    );
    std::fs::remove_dir_all(&root).unwrap();
}
