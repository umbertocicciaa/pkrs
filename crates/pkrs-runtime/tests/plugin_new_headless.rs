//! Integration (§2.3, §9.4): a plugin scaffolded by `pkrs plugin new` into a `pkrs new`
//! project loads without edits, in-process and through `pkrs-runtime --headless`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::process::Command;

use pkrs_cli::plugin::{PluginVars, new_plugin};
use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::Pcg64Rng;
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectOptions};
use pkrs_script::{ScriptHost, ScriptOwner};

/// A `pkrs new` project with `plugin new "My Plugin"` scaffolded and enabled.
fn project_with_new_plugin(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    scaffold(
        Template::OneShot,
        &dir,
        &TemplateVars::for_project("plugin demo").unwrap(),
    )
    .unwrap();
    let vars = PluginVars::for_plugin("My Plugin").unwrap();
    let out = new_plugin(&dir, &vars).unwrap();
    let hint = out.enable_hint.expect("not enabled yet");
    // Enable it exactly as `pkrs plugin new` tells the user to.
    let manifest = dir.join("pkrs.toml");
    let mut toml = fs::read_to_string(&manifest).unwrap();
    toml.push_str(&format!("\n{hint}\n"));
    fs::write(&manifest, toml).unwrap();
    dir
}

#[test]
fn scaffolded_plugin_loads_in_process() {
    let dir = project_with_new_plugin("plugin-new-inproc");
    let project = Project::open(&ProjectOptions {
        dir: dir.clone(),
        ..ProjectOptions::default()
    })
    .unwrap();
    let (mut host, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(1))).unwrap();
    assert_eq!(report.loaded, ["my-plugin"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let table = host.hook_table();
    assert!(table.battle.contains_key("on_damage_calc"), "{table}");
    assert!(table.battle.contains_key("on_end_of_turn"), "{table}");
    assert!(table.events.contains_key("map_enter"), "{table}");
    assert_eq!(
        host.listeners("map_enter")
            .iter()
            .filter(|l| l.owner == ScriptOwner::plugin("my-plugin"))
            .count(),
        1
    );
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn scaffolded_plugin_loads_headless() {
    let dir = project_with_new_plugin("plugin-new-headless");
    let out = Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
        .args(["--headless", "--ticks", "60", "--project"])
        .arg(&dir)
        // `script=debug` shows the scaffold's `map_enter` handler running.
        .env("RUST_LOG", "info,script=debug")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("plugin my-plugin 0.1.0 loaded"), "{stderr}");
    assert!(stderr.contains("plugins loaded"), "{stderr}");
    assert!(stderr.contains("my-plugin: entered start"), "{stderr}");
    assert!(
        !stderr.contains("ERROR") && !stderr.contains("WARN"),
        "the scaffold must run cleanly:\n{stderr}"
    );
    assert!(stderr.contains("TickLimit"), "{stderr}");
    fs::remove_dir_all(&dir).unwrap();
}
