//! Integration (§11, §15.2): a headless warp + battle scenario through the `audio-town`
//! fixture, asserting the audio command sequence the facade sent to `NullAudio`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_audio::BackendCommand;
use pkrs_runtime::project::{Project, ProjectOptions};
use pkrs_runtime::test_harness::{DEFAULT_SCENARIO_SEED, ScenarioDriver, ScenarioOptions};
use pkrs_script::LuaScenario;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/projects/audio-town")
}

fn open() -> Project {
    Project::open(&ProjectOptions {
        dir: fixture(),
        ..ProjectOptions::default()
    })
    .unwrap()
}

#[test]
fn warp_and_battle_scenario_sends_the_expected_audio_commands() {
    let project = open();
    let name = "tests/warp_battle_audio.lua";
    let source = std::fs::read_to_string(fixture().join(name)).unwrap();
    let options = ScenarioOptions::default();
    let mut scenario = LuaScenario::load(name, &source, options.limits).unwrap();
    let mut driver = ScenarioDriver::start(&project, DEFAULT_SCENARIO_SEED).unwrap();
    let report = driver.run(&mut scenario, options.max_ticks);
    assert!(report.passed(), "{report}");

    let host = driver.host();
    assert_eq!(host.audio().backend_name(), "null");
    let log = host.audio_log().expect("headless runs record audio");
    assert_eq!(
        log.lines(),
        [
            "play #1 bgm:town fade=0",
            "stop #1 fade=30",
            "play #2 bgm:house fade=30",
            "stop #2 fade=30",
            "play #3 bgm:town fade=30",
            "play #4 sfx:exclaim fade=0",
            "pause #3 fade=0",
            "play #5 bgm:battle_trainer:intro fade=0",
            "play #6 bgm:battle_trainer fade=0",
            "stop #6 fade=0",
            "play #7 jingle:victory fade=0",
            "resume #3 fade=15",
        ]
    );

    let entries = log.entries();
    // A crossfade stops the old track and starts the new one on the same tick, both over
    // 30 ticks.
    assert_eq!(entries[1].tick, entries[2].tick);
    assert_eq!(entries[3].tick, entries[4].tick);
    // Loop points come from audio/bgm/index.ron.
    match &entries[2].command {
        BackendCommand::Play { source, .. } => {
            assert_eq!(source.path, "audio/bgm/house.ogg");
            let region = source.looping.expect("BGM loops");
            assert_eq!((region.start, region.end), (22_050, Some(882_000)));
        }
        other => panic!("expected the house BGM, got {other}"),
    }
    // The battle starts on the tick the town track pauses; the intro lasts NullAudio's
    // default 60 ticks before the loop takes over.
    assert_eq!(entries[6].tick, entries[7].tick);
    assert_eq!(
        entries[8].tick - entries[7].tick,
        u64::from(pkrs_audio::DEFAULT_NULL_SOUND_TICKS)
    );
    // The town track resumes when the victory jingle ends.
    assert_eq!(
        entries[11].tick - entries[10].tick,
        u64::from(pkrs_audio::DEFAULT_NULL_SOUND_TICKS)
    );
    assert_eq!(host.audio().current_bgm(), Some("town"));
    assert!(!host.audio().in_battle());
}
