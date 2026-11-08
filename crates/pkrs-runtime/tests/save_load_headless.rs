//! Integration (§12): saving the running game to a slot and loading it into a fresh host.
//! World state (flags, variables, bag, per-map state with the active map's NPC changes),
//! player position, RNG streams and plugin save data survive; a function in `plugin.save` is
//! skipped; `scripts/migrate.lua` runs when the save was written by another game version.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{Direction, GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{GameSaveError, Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{SimulationHost, TICK_DT};
use pkrs_save::{RNG_BATTLE, RNG_ENCOUNTER, RNG_SCRIPT, SaveLoader, SaveSlot, SlotInfo, Value};
use pkrs_script::{ScriptHost, ScriptOwner, ScriptValue};

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

/// A `pkrs new` project with the harbor map (§6.5 fixture), the `ledger` plugin (enabled) and
/// `scripts/migrate.lua`.
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
    let manifest = dir.join("pkrs.toml");
    let mut toml = fs::read_to_string(&manifest).unwrap();
    toml.push_str("\n[plugins]\nenabled = [\"ledger\"]\n");
    fs::write(&manifest, toml).unwrap();
    dir
}

fn host(dir: &Path, at: GridPos) -> ProjectHost {
    let project = Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        map: Some("harbor".into()),
        warp: Some(at),
    })
    .unwrap();
    let (scripts, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(55))).unwrap();
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    ProjectHost::with_scripts(&project, renderer, scripts).unwrap()
}

fn tick(host: &mut ProjectHost, keys: &[Key]) {
    let frame = if keys.is_empty() {
        NullFrame::idle()
    } else {
        NullFrame::with_keys(keys.iter().copied())
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([frame])),
        Box::new(FixedPaths::under("save-load")),
        PlatformConfig::default(),
    );
    block_on(platform.next_frame());
    host.poll_input(&platform);
    host.begin_tick();
    host.simulation_tick(TICK_DT);
}

fn idle(host: &mut ProjectHost, ticks: u32) {
    for _ in 0..ticks {
        tick(host, &[]);
    }
}

fn until(host: &mut ProjectHost, max: u32, mut cond: impl FnMut(&ProjectHost) -> bool) {
    for _ in 0..max {
        if cond(host) {
            return;
        }
        tick(host, &[]);
    }
    panic!("condition not met in {max} ticks");
}

fn npc(host: &ProjectHost, id: &str) -> (GridPos, Direction, bool) {
    let scripts = host.scripts().unwrap();
    let w = scripts.world().get();
    let npc = w.npc(id).unwrap();
    (npc.walker.pos(), npc.walker.facing(), npc.visible)
}

fn run(host: &mut ProjectHost, owner: &ScriptOwner, source: &str) -> ScriptValue {
    host.scripts_mut()
        .unwrap()
        .host_mut()
        .run_source(owner, "test.lua", source)
        .unwrap()
}

/// Talks to the sailor (he steps left, the ghost hides, `sailor_met` is set) and walks the
/// player to (6, 4) facing right. Nothing leaves the map, so only the save snapshot's
/// `sync_map_state` records the NPC changes.
fn play(host: &mut ProjectHost) {
    tick(host, &[Key::Up]);
    idle(host, 2);
    tick(host, &[Key::Z]);
    until(host, 50, |h| h.overlay().is_some());
    for _ in 0..50 {
        if host.overlay().is_none() {
            break;
        }
        tick(host, &[Key::Z]);
        tick(host, &[]);
    }
    until(host, 200, |h| {
        npc(h, "sailor").0 == GridPos::new(4, 3) && !h.scripts().unwrap().player_locked()
    });
    for _ in 0..40 {
        if host.walker().pos() == GridPos::new(6, 4) && host.walker().is_idle() {
            break;
        }
        tick(host, &[Key::Right]);
    }
    idle(host, 3);
    assert_eq!(host.walker().pos(), GridPos::new(6, 4));
    run(
        host,
        &ScriptOwner::Game,
        "game.set_var('player_name', 'Robin')",
    );
    run(
        host,
        &ScriptOwner::plugin("ledger"),
        "plugin.save.count = 7; table.insert(plugin.save.history, 'harbor')",
    );
    // Advance the script RNG so its state is not the seed's.
    run(host, &ScriptOwner::Game, "math.random(10); math.random(10)");
}

#[test]
fn save_to_slot_then_load_into_a_fresh_host() {
    let dir = project_dir("save-load");
    let mut a = host(&dir, GridPos::new(5, 4));
    play(&mut a);
    let saved = a.save_to_slot(SaveSlot::Slot(1)).unwrap();

    // The snapshot.
    assert!(dir.join("saves").join("slot1.sav").is_file());
    let world = &saved.world;
    assert!(world.flag("sailor_met"));
    let harbor = world.map("harbor").unwrap();
    assert_eq!(
        harbor.object("1").moved_to,
        Some((GridPos::new(4, 3), Direction::Left)),
        "sync_map_state ran before the snapshot"
    );
    assert!(harbor.object("2").hidden);
    let player = world.player.as_ref().unwrap();
    assert_eq!(
        (player.map.as_str(), player.pos()),
        ("harbor", GridPos::new(6, 4))
    );
    assert_eq!(player.facing, Direction::Right);
    for stream in [RNG_BATTLE, RNG_ENCOUNTER, RNG_SCRIPT] {
        assert!(world.rng.contains_key(stream), "{stream}");
    }
    assert!(world.rng[RNG_SCRIPT].steps > 0);
    let ledger = &saved.plugins["ledger"];
    assert_eq!(ledger.get("count"), Some(&Value::Int(7)));
    assert_eq!(
        ledger.get("history"),
        Some(&Value::List(vec![
            Value::from("start"),
            Value::from("harbor")
        ]))
    );
    assert_eq!(ledger.get("callback"), None, "functions are skipped");
    assert_eq!(saved.header.summary.player_name, "Robin");
    assert_eq!(saved.header.summary.location, "harbor");
    assert_eq!(saved.header.game_version, "0.1.0");
    assert!(saved.header.play_time_ticks > 0);

    // The slot screen reads the header.
    let list = a.save_store().list(&SaveLoader::new());
    assert!(
        matches!(&list[0].1, SlotInfo::Saved { header, .. } if header.summary.player_name == "Robin")
    );
    assert!(matches!(list[1].1, SlotInfo::Empty));

    // Pretend the save was written by game version 0.0.9: `scripts/migrate.lua` runs on load.
    let path = dir.join("saves").join("slot1.sav");
    let text = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        text.replace("\"game_version\": \"0.1.0\"", "\"game_version\": \"0.0.9\""),
    )
    .unwrap();

    // A fresh session somewhere else on the map.
    let mut b = host(&dir, GridPos::new(1, 1));
    assert_eq!(npc(&b, "sailor").0, GridPos::new(5, 3));
    let loaded = b.load_from_slot(SaveSlot::Slot(1)).unwrap();
    assert!(!loaded.from_backup);
    idle(&mut b, 2);

    assert_eq!(b.scene().map().id, "harbor");
    assert_eq!(b.walker().pos(), GridPos::new(6, 4));
    assert_eq!(b.walker().facing(), Direction::Right);
    assert_eq!(
        npc(&b, "sailor"),
        (GridPos::new(4, 3), Direction::Left, true)
    );
    assert!(!npc(&b, "ghost").2, "the ghost stays hidden");
    {
        let scripts = b.scripts().unwrap();
        let w = scripts.world().get();
        assert!(w.state.flag("met_sailor"), "migrate.lua renamed the flag");
        assert!(!w.state.flags().contains_key("sailor_met"));
        assert_eq!(w.state.var("migrated_from"), Some(&Value::from("0.0.9")));
        assert_eq!(w.state.var("player_name"), Some(&Value::from("Robin")));
    }
    let ledger = b.scripts().unwrap().host().plugin_save("ledger").unwrap();
    let ScriptValue::Map(ledger) = ledger else {
        panic!("{ledger:?}")
    };
    assert_eq!(ledger.get("count"), Some(&ScriptValue::Int(7)));
    assert_eq!(
        b.scripts().unwrap().host().rng_state(),
        Some(saved.world.rng[RNG_SCRIPT].clone())
    );
    assert!(b.play_time_ticks() >= saved.header.play_time_ticks);

    // Saving again keeps the restored state and RNG streams.
    let again = b.snapshot().unwrap();
    assert_eq!(again.world.rng, saved.world.rng);
    let (now, before) = (
        again.world.map("harbor").unwrap(),
        saved.world.map("harbor").unwrap(),
    );
    assert!(now.objects().eq(before.objects()));
    assert_eq!(now.var("talks"), before.var("talks"));
    // Loading entered the map again, so its `map_enter` listener counted one more visit.
    assert_eq!(now.var("visits"), Some(&Value::Int(2)));
    assert_eq!(again.header.game_version, "0.1.0");
    fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_saving_during_a_map_transition() {
    let dir = project_dir("save-busy");
    let mut host = host(&dir, GridPos::new(5, 4));
    run(
        &mut host,
        &ScriptOwner::Game,
        "map.warp('start', 5, 7, 'down')",
    );
    until(&mut host, 20, ProjectHost::is_warping);
    assert!(matches!(
        host.save_to_slot(SaveSlot::Autosave),
        Err(GameSaveError::Busy(_))
    ));
    until(&mut host, 120, |h| !h.is_warping());
    host.save_to_slot(SaveSlot::Autosave).unwrap();
    assert!(dir.join("saves").join("autosave.sav").is_file());
    let mut fresh = self::host(&dir, GridPos::new(5, 4));
    fresh.load_from_slot(SaveSlot::Autosave).unwrap();
    assert_eq!(fresh.scene().map().id, "start");
    assert_eq!(fresh.walker().pos(), GridPos::new(5, 7));
    fs::remove_dir_all(&dir).unwrap();
}
