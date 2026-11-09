//! Headless movement from a scripted [`NullGamepad`], and user-config rebinding (§10).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{Direction, GridPos};
use pkrs_platform::{
    Action, BuiltinAction, GamepadButton, NullGamepad, UserBinding, UserConfig, UserPaths,
};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{RuntimeLoopConfig, block_on_loop};
use pkrs_world::MovementConfig;

fn new_project(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    scaffold(
        Template::OneShot,
        &dir,
        &TemplateVars::for_project("demo").unwrap(),
    )
    .unwrap();
    dir
}

fn host(dir: &std::path::Path) -> ProjectHost {
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

#[test]
fn scripted_gamepad_walks_the_player() {
    let dir = new_project("pad-walk");
    let mut host = host(&dir);
    assert_eq!(host.walker().pos(), GridPos::new(5, 7));
    let movement = MovementConfig::default();
    let total = movement.tap_turn_ticks + 2 * movement.walk_step_ticks;
    // The loop polls once before a scripted frame is consumed the way a keyboard
    // `NullPlatform` script is: the first tick is idle, then the pad is held.
    let mut frames = vec![Vec::new()];
    frames.extend(std::iter::repeat_n(
        vec![GamepadButton::DPadRight],
        total as usize - 1,
    ));
    host.set_gamepad(Box::new(NullGamepad::with_frames(frames)));
    let ticks = u64::from(total);
    let mut platform = pkrs_platform::Platform::new(
        Box::new(pkrs_platform::NullPlatform::new()),
        Box::new(pkrs_platform::FixedPaths::under(dir.join("saves"))),
        pkrs_platform::PlatformConfig::default(),
    );
    block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(ticks),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(host.walker().pos(), GridPos::new(7, 7));
    assert_eq!(host.walker().facing(), Direction::Right);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn saved_rebinding_is_what_the_next_host_reads() {
    let dir = new_project("pad-rebind");
    let project = Project::open(&ProjectOptions {
        dir: dir.clone(),
        ..ProjectOptions::default()
    })
    .unwrap();
    let mut config = UserConfig::default();
    config.bindings.insert(
        "confirm".to_owned(),
        UserBinding {
            keys: vec!["t".to_owned()],
            buttons: vec![],
        },
    );
    config.volumes.master = 0.5;
    config.window_scale = 2;
    config.save(project.paths.config_dir()).unwrap();

    let mut host = host(&dir);
    let confirm = Action::Builtin(BuiltinAction::Confirm);
    assert_eq!(host.input().bound_keys(confirm), &[pkrs_platform::Key::T]);
    assert!(host.input().bound_buttons(confirm).is_empty());
    let mut platform = pkrs_platform::Platform::new(
        Box::new(pkrs_platform::NullPlatform::new()),
        Box::new(pkrs_platform::FixedPaths::under(dir.join("saves"))),
        pkrs_platform::PlatformConfig::default(),
    );
    block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(1),
            ..RuntimeLoopConfig::default()
        },
    );
    assert!((host.audio().volume(pkrs_audio::VolumeGroup::Master) - 0.5).abs() < f32::EPSILON);
    std::fs::remove_dir_all(&dir).unwrap();
}
