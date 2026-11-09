//! Headless hot reload: a map script change applies on the next tick (§8.3, issue #58).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_assets::watcher::ReloadAction;
use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{SimulationHost, TICK_DT};
use pkrs_script::{ScriptHost, ScriptOwner};

const MAP_SCRIPT: &str = "scripts/maps/lighthouse_1f.lua";

fn project_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-hot-reload-{}-{nanos}", std::process::id()));
    let vars = TemplateVars::for_project("reload-test").unwrap();
    scaffold(Template::OneShot, &dir, &vars).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cutscene");
    for file in [
        "maps/lighthouse_1f.tmx",
        "scripts/maps/lighthouse_1f.lua",
        "scripts/main.lua",
    ] {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures.join(file), &to).unwrap();
    }
    dir
}

fn tick(host: &mut ProjectHost) {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([NullFrame::idle()])),
        Box::new(FixedPaths::under("hot-reload")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    host.poll_input(&platform);
    host.begin_tick();
    host.simulation_tick(TICK_DT);
}

fn map_marker(host: &mut ProjectHost) -> Option<i64> {
    let scripts = host.scripts_mut()?;
    let owner = ScriptOwner::Map("lighthouse_1f".into());
    scripts
        .host_mut()
        .global(&owner, "RELOAD_MARKER")
        .ok()?
        .as_int()
}

#[test]
fn map_script_reload_picks_up_edits_without_restart() {
    let dir = project_dir();
    let project = Project::open(&ProjectOptions {
        dir: dir.clone(),
        map: Some("lighthouse_1f".into()),
        warp: Some(GridPos::new(2, 2)),
    })
    .unwrap();
    let (scripts, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(7))).unwrap();
    assert!(report.failed.is_empty());
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    let mut host = ProjectHost::with_scripts(&project, renderer, scripts).unwrap();
    host.enable_dev(None, report.loaded);
    tick(&mut host);
    assert_eq!(map_marker(&mut host), None);

    let map_path = dir.join(MAP_SCRIPT);
    let mut source = std::fs::read_to_string(&map_path).unwrap();
    source.push_str("\nRELOAD_MARKER = 42\n");
    std::fs::write(&map_path, source).unwrap();

    host.apply_hot_reload(ReloadAction::Scripts);
    tick(&mut host);
    assert_eq!(map_marker(&mut host), Some(42));

    let snap = host.debug_overlay_snapshot(16.0);
    assert_eq!(snap.map, "lighthouse_1f");
    assert!(snap.ticks > 0);
    assert!(snap.fps > 0.0);
}
