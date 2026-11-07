//! CLI test (§2.3, §9.4): `pkrs plugin new` scaffolds a plugin into a fixture project, and
//! `pkrs check --plugins` loads it without edits and prints the load order and hook table.
//! (`pkrs-runtime --headless` loading it is covered in `pkrs-runtime/tests/plugin_new_headless.rs`.)

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn pkrs(args: &[&str], project: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pkrs"))
        .args(args)
        .arg("--project")
        .arg(project)
        .output()
        .unwrap()
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n")
}

/// A `pkrs new` project.
fn fixture_project(root: &Path) -> PathBuf {
    let dest = root.join("game");
    let out = Command::new(env!("CARGO_BIN_EXE_pkrs"))
        .args(["new", "game", "--path"])
        .arg(&dest)
        .output()
        .unwrap();
    assert!(out.status.success());
    dest
}

#[test]
fn plugin_new_then_check_loads_it_without_edits() {
    let root = temp_dir("plugin-new");
    let project = fixture_project(&root);

    let out = pkrs(&["plugin", "new", "Day Night"], &project);
    let text = stdout(&out);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.contains("plugin `day-night`"), "{text}");
    // Not enabled yet: the CLI prints the line to add instead of editing pkrs.toml (§2.5).
    assert!(
        text.contains("[plugins]\nenabled = [\"day-night\"]"),
        "{text}"
    );
    let dir = project.join("plugins/day-night");
    for file in ["plugin.toml", "init.lua", "README.md"] {
        assert!(dir.join(file).is_file(), "missing {file}");
    }
    let manifest = std::fs::read_to_string(dir.join("plugin.toml")).unwrap();
    assert!(
        manifest.contains("id          = \"day-night\""),
        "{manifest}"
    );
    assert!(
        manifest.contains("name        = \"Day Night\""),
        "{manifest}"
    );
    assert!(!manifest.contains("{{"), "every placeholder is rendered");
    assert!(
        !std::fs::read_to_string(project.join("pkrs.toml"))
            .unwrap()
            .contains("[plugins]"),
        "pkrs.toml is left alone"
    );

    // Not enabled: `check` resolves no plugins.
    let out = pkrs(&["check", "--plugins"], &project);
    assert!(out.status.success());
    assert!(stdout(&out).starts_with("plugins: none enabled\n"));

    // Enable it as the hint says; the scaffold itself is not edited.
    let toml = std::fs::read_to_string(project.join("pkrs.toml")).unwrap();
    std::fs::write(
        project.join("pkrs.toml"),
        format!("{toml}\n[plugins]\nenabled = [\"day-night\"]\n"),
    )
    .unwrap();
    let out = pkrs(&["check", "--plugins"], &project);
    let text = stdout(&out);
    assert!(
        out.status.success(),
        "{text}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.starts_with("plugins (load order): day-night\nbattle hooks:\n"),
        "{text}"
    );
    for line in [
        "  on_damage_calc\n       0  day-night\n",
        "  on_end_of_turn\n       0  day-night\n",
        "world events:\n  map_enter\n       0  day-night\n",
    ] {
        assert!(text.contains(line), "{text}");
    }
    assert!(text.ends_with("check passed\n"), "{text}");

    // A second plugin, now with the first already enabled: the hint keeps it.
    let out = pkrs(&["plugin", "new", "status-pack"], &project);
    assert!(out.status.success());
    assert!(
        stdout(&out).contains("enabled = [\"day-night\", \"status-pack\"]"),
        "{}",
        stdout(&out)
    );

    // Scaffolding over an existing plugin is refused.
    let again = pkrs(&["plugin", "new", "day-night"], &project);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("not empty"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn plugin_new_rejects_bad_names_and_missing_projects() {
    let root = temp_dir("plugin-new-bad");
    let project = fixture_project(&root);
    for bad in ["2fast", "../escape", "a.b"] {
        let out = pkrs(&["plugin", "new", bad], &project);
        assert!(!out.status.success(), "{bad}");
        assert!(
            String::from_utf8_lossy(&out.stderr).contains("invalid plugin name"),
            "{bad}"
        );
    }
    assert!(!project.join("plugins").exists());

    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    let out = pkrs(&["plugin", "new", "x"], &empty);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("no pkrs.toml"));
    std::fs::remove_dir_all(&root).unwrap();
}
