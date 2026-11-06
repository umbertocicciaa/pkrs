//! Integration (§9.4): a fixture project with four enabled plugins (plus one installed but
//! disabled) loads them in the resolved order, each `init.lua` in its own `_ENV`, both
//! in-process and through `pkrs-runtime --headless`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::Pcg64Rng;
use pkrs_runtime::plugins::{PluginStartError, plan_project_plugins, start_project_scripts};
use pkrs_runtime::project::{Project, ProjectOptions};
use pkrs_script::{PluginError, ScriptHost, ScriptOwner, ScriptValue};

/// Expected order: dependencies and `load.after` first, ties by `enabled` order.
const ORDER: [&str; 4] = ["ui-kit", "day-night", "status-pack", "resonance"];
const ENABLED: &str = r#"enabled = ["resonance", "day-night", "status-pack", "ui-kit"]"#;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugin_project")
}

fn open(dir: &Path) -> Project {
    Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        ..ProjectOptions::default()
    })
    .unwrap()
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A `pkrs new` project (so it has a playable map) with the fixture's plugins and `enabled`.
fn playable_project(tag: &str, enabled_line: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    scaffold(
        Template::OneShot,
        &dir,
        &TemplateVars::for_project("plugins").unwrap(),
    )
    .unwrap();
    copy_dir(&fixture().join("plugins"), &dir.join("plugins"));
    let manifest = dir.join("pkrs.toml");
    let mut toml = fs::read_to_string(&manifest).unwrap();
    toml.push_str(&format!("\n[plugins]\n{enabled_line}\n"));
    fs::write(&manifest, toml).unwrap();
    dir
}

#[test]
fn fixture_resolves_to_the_expected_order() {
    let project = open(&fixture());
    let plan = plan_project_plugins(&project).unwrap();
    assert_eq!(plan.ids(), ORDER);
    // Same set, same order, every time.
    for _ in 0..5 {
        assert_eq!(plan_project_plugins(&project).unwrap(), plan);
    }
}

#[test]
fn fixture_plugins_init_in_their_own_env() {
    let project = open(&fixture());
    let (mut host, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(1))).unwrap();
    assert_eq!(report.loaded, ORDER);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    for id in ORDER {
        let owner = ScriptOwner::plugin(id);
        assert_eq!(host.global(&owner, "loaded_as").unwrap(), id.into());
    }
    // `overdrive` is installed but not enabled: never run.
    let overdrive = ScriptOwner::plugin("overdrive");
    assert!(host.global(&overdrive, "loaded_as").unwrap().is_nil());
    assert!(
        host.global(&ScriptOwner::Game, "loaded_as")
            .unwrap()
            .is_nil()
    );
    // `plugin.define()` is bound to the manifest and `require` searched the plugin folder.
    let resonance = ScriptOwner::plugin("resonance");
    let p = host
        .run_source(&resonance, "check", "local P = plugin.define()\nreturn P")
        .unwrap();
    let ScriptValue::Map(p) = p else {
        panic!("P must be a table: {p:?}")
    };
    assert_eq!(p["id"], "resonance".into());
    assert_eq!(p["version"], "1.2.0".into());
    assert_eq!(p["api"], ScriptValue::Int(1));
    let ScriptValue::Map(aura) = &p["aura"] else {
        panic!("aura must be a table")
    };
    assert_eq!(aura["multiplier"], ScriptValue::Number(1.5));
}

#[test]
fn runtime_loads_fixture_plugins_in_order_headless() {
    let dir = playable_project("plugins-headless", ENABLED);
    let out = Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
        .args(["--headless", "--ticks", "5", "--project"])
        .arg(&dir)
        .env("RUST_LOG", "info")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let positions: Vec<usize> = ORDER
        .iter()
        .map(|id| {
            stderr
                .find(&format!("plugin init: {id} "))
                .unwrap_or_else(|| panic!("`{id}` did not init:\n{stderr}"))
        })
        .collect();
    assert!(
        positions.windows(2).all(|w| w[0] < w[1]),
        "wrong init order:\n{stderr}"
    );
    assert!(!stderr.contains("plugin init: overdrive"), "{stderr}");
    assert!(stderr.contains("TickLimit"), "{stderr}");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn invalid_plugin_sets_stop_the_runtime() {
    // Enabling `overdrive` too makes it conflict with `resonance`.
    let dir = playable_project(
        "plugins-conflict",
        r#"enabled = ["resonance", "day-night", "status-pack", "ui-kit", "overdrive"]"#,
    );
    let Err(err) = start_project_scripts(&open(&dir), Box::new(Pcg64Rng::from_seed(1))) else {
        panic!("conflicting plugins must not load")
    };
    let PluginStartError::Plugins(errors) = &err else {
        panic!("expected plugin errors: {err}")
    };
    assert!(
        matches!(errors.errors(), [PluginError::Conflict { .. }]),
        "{err}"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
        .args(["--headless", "--ticks", "5", "--project"])
        .arg(&dir)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success(), "a conflict must fail the run");
    assert!(stderr.contains("conflicts with"), "{stderr}");
    fs::remove_dir_all(&dir).unwrap();
}
