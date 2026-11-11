//! Headless: open start menu, use potion on party member, assert HP (issue #57).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::test_harness::{STATE_MENU, state_name};
use pkrs_runtime::{SimulationHost, TICK_DT, build_test_ui_theme};
use pkrs_script::{ScriptHost, ScriptOwner};

const FIXTURE_FILES: &[&str] = &["maps/harbor.tmx", "scripts/maps/harbor.lua"];
const BATTLE_TEMPLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../pkrs-cli/templates/battle");

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn project_dir() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-menu-{}-{nanos}", std::process::id()));
    let vars = TemplateVars::for_project("harbor").unwrap();
    scaffold(Template::OneShot, &dir, &vars).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/world_state");
    for file in FIXTURE_FILES {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures.join(file), &to).unwrap();
    }
    let manifest = dir.join("pkrs.toml");
    let toml = std::fs::read_to_string(&manifest)
        .unwrap()
        .replace("battles = false", "battles = true")
        .replace("party   = false", "party   = true");
    std::fs::write(manifest, toml).unwrap();
    copy_dir(&Path::new(BATTLE_TEMPLATE).join("data"), &dir.join("data"));
    dir
}

fn host(dir: &Path) -> ProjectHost {
    let project = Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        pak: None,
        map: Some("harbor".into()),
        warp: Some(GridPos::new(1, 1)),
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
    let theme = build_test_ui_theme(host.renderer_mut()).unwrap();
    host.set_ui_theme(theme);
    host
}

fn tick(host: &mut ProjectHost, keys: &[Key]) {
    let frame = if keys.is_empty() {
        NullFrame::idle()
    } else {
        NullFrame::with_keys(keys.iter().copied())
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([frame])),
        Box::new(FixedPaths::under("menu-test")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    host.poll_input(&platform);
    host.begin_tick();
    host.simulation_tick(TICK_DT);
}

fn run(host: &mut ProjectHost, source: &str) {
    let scripts = host.scripts_mut().unwrap();
    let _ = scripts
        .host_mut()
        .run_source(&ScriptOwner::Game, "menu-test.lua", source)
        .unwrap();
}

#[test]
fn open_menu_use_potion_restores_hp() {
    let dir = project_dir();
    let mut host = host(&dir);
    run(&mut host, "game.give_item('potion', 1)");
    host.set_party_hp(0, 5).unwrap();
    let before = {
        let m = &host.party().unwrap().members()[0];
        let pack = host.battle_content().unwrap().pack();
        let sp = pack.species.get(&m.species).unwrap();
        m.current_hp(sp)
    };
    assert_eq!(before, 5);

    tick(&mut host, &[]);
    tick(&mut host, &[Key::Escape]);
    assert_eq!(state_name(&host), STATE_MENU);

    // Root → Bag (Down, Confirm)
    tick(&mut host, &[]);
    tick(&mut host, &[Key::Down]);
    tick(&mut host, &[]);
    tick(&mut host, &[Key::Z]);
    // Pocket → first item
    tick(&mut host, &[]);
    tick(&mut host, &[Key::Z]);
    tick(&mut host, &[]);
    tick(&mut host, &[Key::Z]);
    // Target → first party member
    tick(&mut host, &[]);
    tick(&mut host, &[Key::Z]);

    assert!(host.field_menu().is_none());
    let m = &host.party().unwrap().members()[0];
    let sp = host
        .battle_content()
        .unwrap()
        .pack()
        .species
        .get(&m.species)
        .unwrap();
    assert!(m.current_hp(sp) >= before + 20);

    std::fs::remove_dir_all(&dir).ok();
}
