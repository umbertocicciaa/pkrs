//! Integration (§5.5, §2.2 `[locale]`): the `i18n-town` fixture shows the same dialogue in
//! Italian (its default language) and English. The scenario switches through
//! `game.set_language`; the engine-side setter (`ProjectHost::set_language`) does the same
//! for an options menu, and battle texts follow the active language.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_core::{Direction, GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::overworld::UiRequest;
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::test_harness::{ScenarioOptions, run_scenario};
use pkrs_runtime::{SimulationHost, TICK_DT};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/projects")
        .join(name)
}

fn open(dir: &Path) -> Project {
    Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        ..ProjectOptions::default()
    })
    .unwrap()
}

fn host(project: &Project) -> ProjectHost {
    let (scripts, report) =
        start_project_scripts(project, Box::new(Pcg64Rng::from_seed(54))).unwrap();
    assert!(report.failed.is_empty());
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    ProjectHost::with_scripts(project, renderer, scripts).unwrap()
}

fn tick(host: &mut ProjectHost, keys: &[Key]) {
    let frame = if keys.is_empty() {
        NullFrame::idle()
    } else {
        NullFrame::with_keys(keys.iter().copied())
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([frame])),
        Box::new(FixedPaths::under("localization")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    host.poll_input(&platform);
    host.begin_tick();
    host.simulation_tick(TICK_DT);
}

/// Reads the sign right of the start tile and returns the text shown, then closes it.
fn read_sign(host: &mut ProjectHost) -> String {
    tick(host, &[Key::Right]);
    for _ in 0..20 {
        tick(host, &[]);
    }
    assert_eq!(host.walker().facing(), Direction::Right);
    assert_eq!(host.walker().pos(), GridPos::new(5, 7));
    tick(host, &[Key::Z]);
    let shown = match host.overlay().map(|o| o.request().clone()) {
        Some(UiRequest::Say(text)) => text,
        other => panic!("expected a text box, got {other:?}"),
    };
    for _ in 0..200 {
        if host.overlay().is_none() {
            break;
        }
        tick(host, &[Key::Z]);
        tick(host, &[]);
    }
    assert!(host.overlay().is_none(), "the sign closes");
    shown
}

#[test]
fn headless_dialogue_in_italian_and_english_with_a_runtime_switch() {
    let dir = fixture("i18n-town");
    let report = run_scenario(
        &open(&dir),
        &dir.join("tests/switch_language.lua"),
        "tests/switch_language.lua",
        &ScenarioOptions::default(),
    )
    .unwrap();
    assert!(report.passed(), "{report}");
}

#[test]
fn engine_side_setter_switches_the_language_for_later_texts() {
    let project = open(&fixture("i18n-town"));
    let mut host = host(&project);
    assert_eq!(host.language(), "it");
    assert_eq!(read_sign(&mut host), "Cartello: solo in italiano.");
    let err = host.set_language("fr").unwrap_err();
    assert_eq!(err.to_string(), "unknown language `fr` (available: it, en)");
    host.set_language("en").unwrap();
    assert_eq!(host.language(), "en");
    assert_eq!(host.localizer().language(), "en");
    assert_eq!(
        host.localizer().text(
            "text:guide.hello",
            &pkrs_data::TextArgs::new().with("town", "X")
        ),
        "Welcome to X!"
    );
    assert_eq!(
        read_sign(&mut host),
        "Cartello: solo in italiano.",
        "a key English lacks falls back to the default language"
    );
}

#[test]
fn battle_texts_follow_the_active_language() {
    // battle-town declares only English: engine battle texts stay English.
    let project = open(&fixture("battle-town"));
    let host = host(&project);
    assert_eq!(host.language(), "en");
    let text = pkrs_runtime::locale::BattleText::new(host.localizer(), None);
    assert_eq!(
        text.say("battle.go", &[("name", "Leafkit")]),
        "Go! Leafkit!"
    );
    // A project whose languages include Italian gets the shipped Italian engine texts.
    let mut it = host.localizer();
    let mut localizer = pkrs_data::Localizer::new("en", vec!["it".into()]);
    for layer in it.layers() {
        localizer.push_layer(layer.clone());
    }
    localizer.set_language("it").unwrap();
    it = localizer;
    let text = pkrs_runtime::locale::BattleText::new(it, None);
    assert_eq!(
        text.say("battle.go", &[("name", "Leafkit")]),
        "Vai! Leafkit!"
    );
    assert_eq!(
        pkrs_runtime::battle::message_text_in(&text, "multi_hit:2", &Default::default()),
        "Colpito altre 2 volte!"
    );
}
