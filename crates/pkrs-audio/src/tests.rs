//! Facade policy over [`NullAudio`] (§11): crossfades, intros, ducking, battle transitions,
//! instance limits, volume groups and focus ducking.

use super::*;

const MANIFEST: &str = r#"(
    bgm: {
        "town": (loop_start: 1000),
        "battle_wild": (intro: "battle_wild_intro.ogg"),
    },
    jingles: {
        "item_get": (mode: Duck),
        "level_up": (mode: Pause),
        "victory": (mode: Pause),
    },
    sfx: { "bump": (max_instances: 2) },
    battle: (wild: "battle_wild", victory: "victory"),
)"#;

fn manifest() -> AudioManifest {
    AudioManifest::parse(MANIFEST, "audio/bgm/index.ron", "audio").unwrap()
}

fn audio_with(null: NullAudio, config: AudioConfig) -> (Audio, AudioLog) {
    let log = null.log();
    (Audio::new(Box::new(null), manifest(), config), log)
}

fn audio() -> (Audio, AudioLog) {
    let null = NullAudio::new()
        .with_length("battle_wild:intro", 5)
        .with_length("item_get", 3)
        .with_length("level_up", 4)
        .with_length("victory", 8)
        .with_length("bump", 10);
    audio_with(null, AudioConfig::default())
}

/// Lines logged since the last call.
fn drain(log: &AudioLog) -> Vec<String> {
    let lines = log.lines();
    log.clear();
    lines
}

fn flush_n(audio: &mut Audio, n: u32) {
    for _ in 0..n {
        audio.flush();
    }
}

#[test]
fn requests_reach_the_backend_only_on_flush() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.play_sfx("bump");
    assert!(log.is_empty());
    audio.flush();
    assert_eq!(
        drain(&log),
        ["play #1 bgm:town fade=0", "play #2 sfx:bump fade=0"]
    );
    assert_eq!(audio.current_bgm(), Some("town"));
    assert_eq!(audio.ticks(), 1);
}

#[test]
fn same_bgm_keeps_playing_and_another_crossfades_over_30_ticks() {
    let (mut audio, log) = audio();
    audio.map_bgm(Some("town"));
    audio.flush();
    assert_eq!(drain(&log), ["play #1 bgm:town fade=0"]);

    audio.map_bgm(Some("town"));
    audio.map_bgm(None);
    audio.flush();
    assert!(drain(&log).is_empty(), "same id must keep playing");

    audio.map_bgm(Some("route1"));
    audio.flush();
    assert_eq!(
        drain(&log),
        ["stop #1 fade=30", "play #2 bgm:route1 fade=30"]
    );
    assert_eq!(audio.current_bgm(), Some("route1"));

    let entries = {
        audio.play_bgm("cave", Some(12));
        audio.flush();
        log.entries()
    };
    assert_eq!(
        entries
            .iter()
            .map(|e| e.command.to_string())
            .collect::<Vec<_>>(),
        ["stop #2 fade=12", "play #3 bgm:cave fade=12"]
    );
    match &entries[1].command {
        BackendCommand::Play { source, .. } => {
            assert_eq!(source.path, "audio/bgm/cave.ogg");
            assert_eq!(source.kind, SoundKind::Stream);
            assert_eq!(source.looping, Some(LoopRegion::default()));
        }
        other => panic!("expected play, got {other}"),
    }
}

#[test]
fn loop_points_come_from_the_manifest() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.flush();
    match &log.entries()[0].command {
        BackendCommand::Play { source, .. } => assert_eq!(
            source.looping,
            Some(LoopRegion {
                start: 1000,
                end: None
            })
        ),
        other => panic!("expected play, got {other}"),
    }
}

#[test]
fn stop_bgm_fades_out() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.stop_bgm(None);
    audio.stop_bgm(Some(5));
    audio.flush();
    assert_eq!(drain(&log), ["play #1 bgm:town fade=0", "stop #1 fade=30"]);
    assert_eq!(audio.current_bgm(), None);
}

#[test]
fn an_intro_plays_once_then_the_loop_starts() {
    let (mut audio, log) = audio();
    audio.play_bgm("battle_wild", None);
    audio.flush();
    assert_eq!(drain(&log), ["play #1 bgm:battle_wild:intro fade=0"]);
    flush_n(&mut audio, 4);
    assert!(drain(&log).is_empty());
    audio.flush();
    let entries = log.entries();
    assert_eq!(drain(&log), ["play #2 bgm:battle_wild fade=0"]);
    assert_eq!(
        entries[0].tick, 5,
        "the loop starts on the tick the intro ends"
    );
    match &entries[0].command {
        BackendCommand::Play { source, .. } => assert!(source.looping.is_some()),
        other => panic!("expected play, got {other}"),
    }
    assert_eq!(audio.current_bgm(), Some("battle_wild"));
}

#[test]
fn a_duck_jingle_lowers_then_restores_the_bgm() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.flush();
    drain(&log);

    audio.play_jingle("item_get");
    audio.flush();
    assert_eq!(
        drain(&log),
        ["volume #1 0.30 fade=6", "play #2 jingle:item_get fade=0"]
    );
    assert!(audio.jingle_playing());
    // A map change while ducked starts the new track ducked too.
    audio.play_bgm("route1", None);
    audio.flush();
    assert_eq!(
        drain(&log),
        ["stop #1 fade=30", "play #3 bgm:route1 fade=30 vol=0.30"]
    );
    // Played on tick 0 for 3 ticks: the BGM comes back on tick 3.
    audio.flush();
    assert!(drain(&log).is_empty());
    audio.flush();
    let entries = log.entries();
    assert_eq!(drain(&log), ["volume #3 1.00 fade=6"]);
    assert_eq!(entries[0].tick, 4);
    assert!(!audio.jingle_playing());
}

#[test]
fn a_pause_jingle_pauses_then_resumes_the_bgm() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.play_jingle("level_up");
    audio.flush();
    assert_eq!(
        drain(&log),
        [
            "play #1 bgm:town fade=0",
            "pause #1 fade=0",
            "play #2 jingle:level_up fade=0"
        ]
    );
    flush_n(&mut audio, 3);
    assert!(drain(&log).is_empty());
    audio.flush();
    assert_eq!(drain(&log), ["resume #1 fade=15"]);
}

#[test]
fn a_new_jingle_replaces_the_running_one_and_keeps_the_hold() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.play_jingle("item_get");
    audio.flush();
    drain(&log);
    audio.play_jingle("level_up");
    audio.flush();
    assert_eq!(
        drain(&log),
        [
            "stop #2 fade=0",
            "pause #1 fade=0",
            "play #3 jingle:level_up fade=0"
        ]
    );
    flush_n(&mut audio, 4);
    assert_eq!(drain(&log), ["resume #1 fade=15"]);
}

#[test]
fn sfx_instances_are_limited_per_id() {
    let (mut audio, log) = audio();
    audio.play_sfx("bump");
    audio.play_sfx("bump");
    audio.play_sfx("door");
    audio.play_sfx("bump");
    audio.flush();
    assert_eq!(
        drain(&log),
        [
            "play #1 sfx:bump fade=0",
            "play #2 sfx:bump fade=0",
            "play #3 sfx:door fade=0",
            "stop #1 fade=0",
            "play #4 sfx:bump fade=0",
        ]
    );
}

#[test]
fn finished_sfx_do_not_count_against_the_limit() {
    let (mut audio, log) = audio();
    audio.play_sfx("bump");
    audio.play_sfx("bump");
    audio.flush();
    flush_n(&mut audio, 10);
    drain(&log);
    audio.play_sfx("bump");
    audio.play_sfx("bump");
    audio.flush();
    assert_eq!(
        drain(&log),
        ["play #3 sfx:bump fade=0", "play #4 sfx:bump fade=0"]
    );
    match &AudioManifest::default().resolve_sfx("x", 4).0.kind {
        SoundKind::Static => {}
        SoundKind::Stream => panic!("sfx must be static"),
    }
}

#[test]
fn a_won_battle_plays_the_victory_jingle_then_resumes_the_overworld_track() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.flush();
    drain(&log);

    audio.start_battle(BattleKind::Wild);
    audio.flush();
    assert_eq!(
        drain(&log),
        ["pause #1 fade=0", "play #2 bgm:battle_wild:intro fade=0"]
    );
    assert!(audio.in_battle());
    flush_n(&mut audio, 5);
    assert_eq!(drain(&log), ["play #3 bgm:battle_wild fade=0"]);

    audio.end_battle(BattleKind::Wild, true);
    audio.flush();
    assert_eq!(
        drain(&log),
        ["stop #3 fade=0", "play #4 jingle:victory fade=0"]
    );
    assert!(!audio.in_battle());
    flush_n(&mut audio, 7);
    assert!(drain(&log).is_empty());
    audio.flush();
    assert_eq!(drain(&log), ["resume #1 fade=15"]);
    assert_eq!(audio.current_bgm(), Some("town"));
}

#[test]
fn a_lost_battle_fades_out_and_resumes_at_once() {
    let (mut audio, log) = audio();
    audio.play_bgm("town", None);
    audio.start_battle_with(Some("gym"));
    audio.end_battle(BattleKind::Trainer, false);
    audio.flush();
    assert_eq!(
        drain(&log),
        [
            "play #1 bgm:town fade=0",
            "pause #1 fade=0",
            "play #2 bgm:gym fade=0",
            "stop #2 fade=10",
            "resume #1 fade=15",
        ]
    );
}

#[test]
fn restart_after_battle_starts_the_track_again() {
    let config = AudioConfig {
        battle_resume: BattleResume::Restart,
        ..AudioConfig::default()
    };
    let (mut audio, log) = audio_with(NullAudio::new(), config);
    audio.play_bgm("town", None);
    audio.start_battle_with(Some("gym"));
    audio.end_battle_with(None);
    audio.flush();
    assert_eq!(
        drain(&log)[3..],
        [
            "stop #2 fade=10",
            "stop #1 fade=0",
            "play #3 bgm:town fade=15"
        ]
    );
}

#[test]
fn volume_groups_and_focus_ducking() {
    let (mut audio, log) = audio();
    audio.set_volume(VolumeGroup::Bgm, 0.5);
    audio.set_volume(VolumeGroup::Sfx, 7.0);
    audio.set_focus(false);
    audio.set_focus(false);
    audio.flush();
    assert_eq!(
        drain(&log),
        [
            "group bgm 0.50 fade=0",
            "group sfx 1.00 fade=0",
            "group master 0.25 fade=6"
        ]
    );
    audio.set_volume(VolumeGroup::Master, 0.8);
    audio.set_focus(true);
    audio.flush();
    assert_eq!(
        drain(&log),
        ["group master 0.20 fade=0", "group master 0.80 fade=6"]
    );
    assert!((audio.volume(VolumeGroup::Bgm) - 0.5).abs() < f32::EPSILON);
}

#[test]
fn null_constructor_and_debug_are_usable() {
    let (mut audio, log) = Audio::null(AudioManifest::default(), AudioConfig::default());
    assert_eq!(audio.backend_name(), "null");
    audio.play_bgm("town", None);
    audio.flush();
    assert_eq!(log.len(), 1);
    assert!(format!("{audio:?}").contains("town"));
}
