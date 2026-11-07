//! Integration (§2.3, §2.4): a project made by `pkrs new` runs headless through
//! `pkrs-runtime --project`, and the player can walk on it.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{Direction, GridPos};
use pkrs_platform::{
    FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, UserPaths,
};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{RuntimeLoopConfig, block_on_loop};
use pkrs_world::MovementConfig;

fn new_project(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    let vars = TemplateVars::for_project("demo").unwrap();
    scaffold(Template::OneShot, &dir, &vars).unwrap();
    dir
}

fn runtime(project: &Path, extra: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_pkrs-runtime"))
        .args(["--headless", "--ticks", "60", "--project"])
        .arg(project)
        .args(extra)
        .env("RUST_LOG", "info")
        .output()
        .unwrap()
}

#[test]
fn new_project_runs_headless_for_60_ticks() {
    let dir = new_project("headless");
    let out = runtime(&dir, &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("map_enter"), "{stderr}");
    assert!(stderr.contains("TickLimit"), "{stderr}");

    let warped = runtime(&dir, &["--map", "start", "--warp", "3,3"]);
    assert!(
        warped.status.success(),
        "{}",
        String::from_utf8_lossy(&warped.stderr)
    );

    let outside = runtime(&dir, &["--warp", "500,500"]);
    assert!(!outside.status.success(), "an off-map warp must fail");
    let missing = runtime(&dir, &["--map", "nowhere"]);
    assert!(!missing.status.success(), "a missing map must fail");
    std::fs::remove_dir_all(&dir).unwrap();
}

fn host(dir: &Path) -> ProjectHost {
    let project = Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        ..ProjectOptions::default()
    })
    .unwrap();
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    ProjectHost::new(&project, renderer).unwrap()
}

fn ticks_for_held_walk_steps(steps: u32) -> u32 {
    let c = MovementConfig::default();
    if steps == 0 {
        return 0;
    }
    c.tap_turn_ticks + steps * c.walk_step_ticks
}

fn hold(host: &mut ProjectHost, dir: &Path, key: Key, steps: u32) {
    let total = ticks_for_held_walk_steps(steps);
    let frames = std::iter::repeat_n(NullFrame::with_keys([key]), total as usize);
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(frames)),
        Box::new(FixedPaths::under(dir.join("saves"))),
        PlatformConfig::default(),
    );
    block_on_loop(
        &mut platform,
        host,
        &RuntimeLoopConfig {
            max_ticks: Some(u64::from(total)),
            ..RuntimeLoopConfig::default()
        },
    );
}

fn run_idle_ticks(host: &mut ProjectHost, dir: &Path, ticks: u32) {
    let frames = std::iter::repeat_n(NullFrame::idle(), ticks as usize);
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(frames)),
        Box::new(FixedPaths::under(dir.join("saves"))),
        PlatformConfig::default(),
    );
    block_on_loop(
        &mut platform,
        host,
        &RuntimeLoopConfig {
            max_ticks: Some(u64::from(ticks)),
            ..RuntimeLoopConfig::default()
        },
    );
}

#[test]
fn the_template_map_is_walkable_with_collision() {
    let dir = new_project("walk");
    let mut host = host(&dir);
    assert_eq!(
        host.walker().pos(),
        GridPos::new(5, 7),
        "start.position from pkrs.toml"
    );
    assert_eq!(host.scene().map().id, "start");

    hold(&mut host, &dir, Key::Right, 2);
    assert_eq!(host.walker().pos(), GridPos::new(7, 7));
    assert_eq!(host.walker().facing(), Direction::Right);

    // Walking up eventually hits the solid tree border, so the player stops on row 1.
    hold(&mut host, &dir, Key::Up, 10);
    assert_eq!(
        host.walker().pos(),
        GridPos::new(7, 1),
        "blocked by the tree border"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn walking_north_across_seam_swaps_map_without_transition() {
    let dir = new_project("seamless");
    let mut host = host(&dir);
    assert_eq!(host.scene().map().id, "start");

    // Stand on the north edge; one more step crosses into `maps/north.tmx` (template `world.ron`).
    let project = Project::open(&ProjectOptions {
        dir: dir.clone(),
        warp: Some(GridPos::new(5, 0)),
        ..ProjectOptions::default()
    })
    .unwrap();
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    host = ProjectHost::new(&project, renderer).unwrap();
    assert_eq!(host.walker().pos(), GridPos::new(5, 0));

    hold(&mut host, &dir, Key::Up, 1);
    assert_eq!(
        host.scene().map().id,
        "north",
        "seamless crossing should swap the active map"
    );
    assert_eq!(
        host.walker().pos(),
        GridPos::new(5, 5),
        "entry tile on the north map"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn stepping_onto_warp_changes_map_after_fade() {
    let dir = new_project("warp-step");
    let mut host = host(&dir);
    assert_eq!(host.scene().map().id, "start");

    // Template warp tile is at (6, 10); start is (5, 7). Walk Down three tiles while holding.
    let c = MovementConfig::default();
    let steps: u32 = 3;
    let walk_ticks = c.tap_turn_ticks + (steps - 1) * c.walk_step_ticks;
    hold(&mut host, &dir, Key::Down, steps);
    assert_eq!(host.walker().pos(), GridPos::new(5, 10));

    // Fade out + fade in (20 ticks each, §6.4 default in ProjectHost).
    run_idle_ticks(&mut host, &dir, 40);
    assert_eq!(
        host.scene().map().id,
        "dest",
        "warp should load maps/dest.tmx"
    );
    assert_eq!(host.walker().pos(), GridPos::new(5, 7));
    assert_eq!(walk_ticks, 40, "sanity: held walk tick count for 3 steps");

    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn dev_saves_go_under_the_project() {
    let dir = new_project("saves");
    let project = Project::open(&ProjectOptions {
        dir: dir.clone(),
        ..ProjectOptions::default()
    })
    .unwrap();
    assert_eq!(project.paths.data_dir(), dir.join("saves"));
    std::fs::remove_dir_all(&dir).unwrap();
}
