//! Title → Continue → load slot 1 restores a save written in a prior session (§12).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectOptions, ProjectSessionHost};
use pkrs_runtime::{SimulationHost, TICK_DT};
use pkrs_save::{SaveLoader, SaveSlot, SlotInfo};
use pkrs_script::ScriptHost;

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

fn project_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    scaffold(
        Template::OneShot,
        &dir,
        &TemplateVars::for_project("harbor").unwrap(),
    )
    .unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    copy_dir(&fixtures.join("world_state"), &dir);
    copy_dir(&fixtures.join("save_game"), &dir);
    dir
}

fn session(dir: &Path, skip_title: bool, warp: Option<GridPos>) -> ProjectSessionHost {
    let project = Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        pak: None,
        map: Some("harbor".into()),
        warp,
    })
    .unwrap();
    let (scripts, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(77))).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    ProjectSessionHost::open(&project, renderer, scripts, skip_title).unwrap()
}

fn tick(host: &mut ProjectSessionHost, keys: &[Key]) {
    let frame = if keys.is_empty() {
        NullFrame::idle()
    } else {
        NullFrame::with_keys(keys.iter().copied())
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([frame])),
        Box::new(FixedPaths::under("title-continue")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    host.poll_input(&platform);
    host.begin_tick();
    host.simulation_tick(TICK_DT);
}

fn press_menu_down_confirm(host: &mut ProjectSessionHost, extra_down: u32) {
    for _ in 0..extra_down {
        tick(host, &[Key::Down]);
    }
    tick(host, &[Key::Z]);
}

#[test]
fn save_slot_one_then_continue_from_title() {
    let dir = project_dir("title-continue");
    let mut play = session(&dir, true, Some(GridPos::new(5, 4)));
    assert!(play.overworld_mut().is_some());
    let inner = play.host_mut().expect("overworld");
    inner
        .scripts_mut()
        .unwrap()
        .host_mut()
        .run_source(
            &pkrs_script::ScriptOwner::Game,
            "t.lua",
            "game.set_var('player_name', 'Robin')",
        )
        .unwrap();
    inner.save_to_slot(SaveSlot::Slot(1)).unwrap();
    assert!(dir.join("saves").join("slot1.sav").is_file());
    let project = Project::open(&ProjectOptions {
        dir: dir.clone(),
        pak: None,
        map: Some("harbor".into()),
        warp: None,
    })
    .unwrap();
    let list = pkrs_runtime::project::save_store(&project.paths).list(&SaveLoader::new());
    assert!(
        list.iter()
            .any(|(_, info)| matches!(info, SlotInfo::Saved { .. })),
        "slot list before title: {list:?}"
    );

    let mut menu = session(&dir, false, None);
    assert_eq!(menu.front_screen(), Some("title"));
    assert!(menu.overworld().is_none());
    press_menu_down_confirm(&mut menu, 1);
    assert_eq!(menu.front_screen(), Some("save"));
    tick(&mut menu, &[]);
    tick(&mut menu, &[Key::Down]);
    tick(&mut menu, &[Key::Z]);
    for _ in 0..120 {
        tick(&mut menu, &[]);
    }
    assert!(
        menu.overworld().is_some(),
        "screen={:?}",
        menu.front_screen()
    );
    let world = menu.host().expect("overworld").walker().pos();
    assert_eq!(world, GridPos::new(5, 4));
    assert_eq!(
        menu.host()
            .expect("overworld")
            .scripts()
            .unwrap()
            .world()
            .get()
            .state
            .var("player_name"),
        Some(&pkrs_save::Value::from("Robin"))
    );
    fs::remove_dir_all(&dir).unwrap();
}
