//! Integration (§9.4, §6.3, §10, §13): a fixture plugin adds a move (data folder and
//! `data.register_move`), a terrain rule (`world.register_terrain("mud", …)`) and field
//! actions, used in a headless overworld: mud blocks the player until the party-menu field
//! action puts waders on; Confirm facing mud runs the tile action; a custom input action
//! fires its event; plugin assets are mounted under `mudworks:`; `plugin.save` holds data.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{Direction, GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::plugins::{PluginStartError, project_assets, start_project_scripts};
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{SimulationHost, TICK_DT};
use pkrs_script::{PluginLoadReport, ScriptValue};

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/plugin_content")
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

/// A `pkrs new` project plus the fixture map, tileset and `mudworks` plugin (enabled).
fn project_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    scaffold(
        Template::OneShot,
        &dir,
        &TemplateVars::for_project("mudflats").unwrap(),
    )
    .unwrap();
    copy_dir(&fixture(), &dir);
    let manifest = dir.join("pkrs.toml");
    let mut toml = fs::read_to_string(&manifest).unwrap();
    toml.push_str("\n[plugins]\nenabled = [\"mudworks\"]\n");
    fs::write(&manifest, toml).unwrap();
    dir
}

fn open(dir: &Path) -> Project {
    Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        pak: None,
        map: Some("mudflat".into()),
        warp: Some(GridPos::new(1, 1)),
    })
    .unwrap()
}

fn start(dir: &Path) -> (ProjectHost, PluginLoadReport, Project) {
    let project = open(dir);
    let (scripts, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(46))).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    let host = ProjectHost::with_scripts(&project, renderer, scripts).unwrap();
    (host, report, project)
}

fn tick(host: &mut ProjectHost, keys: &[Key]) {
    let frame = if keys.is_empty() {
        NullFrame::idle()
    } else {
        NullFrame::with_keys(keys.iter().copied())
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([frame])),
        Box::new(FixedPaths::under("plugin-content")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    host.poll_input(&platform);
    host.begin_tick();
    host.simulation_tick(TICK_DT);
}

/// Holds `key` for `ticks` ticks, then lets go until the walker is idle.
fn hold(host: &mut ProjectHost, key: Key, ticks: u32) {
    for _ in 0..ticks {
        tick(host, &[key]);
    }
    for _ in 0..64 {
        if host.walker().is_idle() {
            break;
        }
        tick(host, &[]);
    }
}

/// Taps `key` (one tick down, a few up so tasks and events run).
fn tap(host: &mut ProjectHost, key: Key) {
    tick(host, &[key]);
    for _ in 0..3 {
        tick(host, &[]);
    }
}

#[test]
fn plugin_move_terrain_and_field_action_in_a_headless_scenario() {
    let dir = project_dir("plugin-content");
    let (mut host, report, project) = start(&dir);

    // The move: one from the plugin's data folder, one from `data.register_move`.
    assert_eq!(report.content_files, 1);
    let scripts = host.scripts().unwrap();
    scripts.host().with_content(|db| {
        let slap = db
            .move_def("mud_slap")
            .expect("data/moves/mud_slap.ron merged");
        assert_eq!(slap.power, 20);
        assert_eq!(db.move_def("mudworks:mud_bomb").unwrap().power, 65);
    });
    assert_eq!(scripts.host().terrain_rules(), ["mud"]);

    // Mud blocks the player: holding Right only turns them.
    assert_eq!(host.walker().pos(), GridPos::new(1, 1));
    hold(&mut host, Key::Right, 40);
    assert_eq!(host.walker().pos(), GridPos::new(1, 1));
    assert_eq!(host.walker().facing(), Direction::Right);

    // Facing mud, Confirm runs the tile-bound field action.
    tap(&mut host, Key::Z);
    let probed =
        ScriptValue::from_save_value(host.scripts().unwrap().world().get().state.var("probed"));
    assert_eq!(probed, ScriptValue::from("2,1"));

    // The party menu offers the waders (not the tile-only probe) and uses them.
    let menu: Vec<String> = host
        .scripts()
        .unwrap()
        .party_field_actions()
        .into_iter()
        .map(|a| a.id)
        .collect();
    assert_eq!(menu, ["mudworks:waders"]);
    let scripts = host.scripts_mut().unwrap();
    assert!(!scripts.use_field_action("mudworks:probe"));
    assert!(scripts.use_field_action("mudworks:waders"));
    tick(&mut host, &[]);

    // Now the terrain rule lets the player wade through the mud.
    hold(&mut host, Key::Right, 60);
    assert!(
        host.walker().pos().x >= 3,
        "player should cross the mud, got {}",
        host.walker().pos()
    );
    assert!(host.walker().field().is_active("mudworks:waders"));

    // A custom input action registered by the plugin is bound and emits `action`.
    assert!(
        host.custom_actions()
            .iter()
            .any(|(name, _)| name == "mudworks:whistle")
    );
    tap(&mut host, Key::M);
    assert!(host.scripts().unwrap().world().get().state.flag("whistled"));

    // `plugin.save` holds the plugin's data (persisting it is §12 / M6).
    let save = host
        .scripts()
        .unwrap()
        .host()
        .plugin_save("mudworks")
        .unwrap();
    let ScriptValue::Map(save) = save else {
        panic!("save must be a table")
    };
    assert_eq!(save["waders_used"], ScriptValue::Int(1));

    // Plugin assets are mounted under `mudworks:`.
    let assets = project_assets(&project, &report.loaded).unwrap();
    assert_eq!(assets.read("mudworks:ui/icon.png").unwrap(), b"icon");
    assert!(assets.mounts().any(|m| m == "mudworks"));

    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn duplicate_plugin_content_without_override_stops_start_up() {
    let dir = project_dir("plugin-content-dup");
    let dup = dir.join("data/moves");
    fs::create_dir_all(&dup).unwrap();
    fs::copy(
        fixture().join("plugins/mudworks/data/moves/mud_slap.ron"),
        dup.join("mud_slap.ron"),
    )
    .unwrap();
    let Err(err) = start_project_scripts(&open(&dir), Box::new(Pcg64Rng::from_seed(1))) else {
        panic!("a duplicate id without override must not load")
    };
    let PluginStartError::Content(errors) = &err else {
        panic!("expected content errors: {err}")
    };
    assert_eq!(errors.0.len(), 1);
    assert!(
        err.to_string().contains(
            "plugins/mudworks/data/moves/mud_slap.ron: duplicate move `mud_slap`: already defined by the game"
        ),
        "{err}"
    );

    // With `override: true` the plugin's record replaces the game's (and is logged).
    let path = dir.join("plugins/mudworks/data/moves/mud_slap.ron");
    let text = fs::read_to_string(&path)
        .unwrap()
        .replace("priority: 0,", "priority: 0,\n    override: true,");
    fs::write(&path, text).unwrap();
    let (scripts, report) =
        start_project_scripts(&open(&dir), Box::new(Pcg64Rng::from_seed(1))).unwrap();
    assert_eq!(report.content_overrides.len(), 1);
    assert_eq!(
        report.content_overrides[0].to_string(),
        "move `mud_slap` from the game overridden by plugin `mudworks`"
    );
    scripts.with_content(|db| assert!(db.move_def("mud_slap").is_some()));
    fs::remove_dir_all(&dir).unwrap();
}
