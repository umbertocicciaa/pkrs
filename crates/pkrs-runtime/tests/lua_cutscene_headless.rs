//! Integration (§9.1–§9.3, §6.4): a headless scripted cutscene on a fixture map. Talking to
//! the keeper runs the §9.3 `old_keeper` script unmodified: the NPC turns, a dialogue and a
//! yes/no choice open, the NPC walks its path, and a flag and an item are set. A second test
//! covers engine events, triggers and per-map script unload on a warp.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use pkrs_cli::template::{Template, TemplateVars, scaffold};
use pkrs_core::{Direction, GridPos, Pcg64Rng};
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};
use pkrs_render::{NullRenderer, Renderer, RendererConfig};
use pkrs_runtime::overworld::UiRequest;
use pkrs_runtime::plugins::start_project_scripts;
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::{SimulationHost, TICK_DT};
use pkrs_script::{ScriptHost, ScriptOwner, ScriptValue};

const FIXTURE_FILES: &[&str] = &[
    "maps/lighthouse_1f.tmx",
    "scripts/maps/lighthouse_1f.lua",
    "scripts/main.lua",
];

/// A `pkrs new` project plus the cutscene fixture files.
fn project_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    let vars = TemplateVars::for_project("lighthouse").unwrap();
    scaffold(Template::OneShot, &dir, &vars).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/cutscene");
    for file in FIXTURE_FILES {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures.join(file), &to).unwrap();
    }
    dir
}

/// Starts on the lighthouse map at `at`, with the project's scripts.
fn host(dir: &Path, at: GridPos) -> ProjectHost {
    let project = Project::open(&ProjectOptions {
        dir: dir.to_path_buf(),
        map: Some("lighthouse_1f".into()),
        warp: Some(at),
    })
    .unwrap();
    let (scripts, report) =
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(28))).unwrap();
    assert!(report.failed.is_empty());
    let renderer = Renderer::new(
        Box::new(NullRenderer::new()),
        project.display(),
        RendererConfig::default(),
    );
    ProjectHost::with_scripts(&project, renderer, scripts).unwrap()
}

/// One simulation tick with `keys` held during its frame.
fn tick(host: &mut ProjectHost, keys: &[Key]) {
    let frame = if keys.is_empty() {
        NullFrame::idle()
    } else {
        NullFrame::with_keys(keys.iter().copied())
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([frame])),
        Box::new(FixedPaths::under("lua-cutscene")),
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

fn overlay(host: &ProjectHost) -> Option<UiRequest> {
    host.overlay().map(|o| o.request().clone())
}

fn say(text: &str) -> Option<UiRequest> {
    Some(UiRequest::Say(text.into()))
}

/// Presses Confirm until the open text box closes.
fn close_text_box(host: &mut ProjectHost) {
    let shown = overlay(host);
    assert!(matches!(shown, Some(UiRequest::Say(_))), "{shown:?}");
    for _ in 0..50 {
        tick(host, &[Key::Z]);
        if overlay(host) != shown {
            return;
        }
        tick(host, &[]);
        if overlay(host) != shown {
            return;
        }
    }
    panic!("text box {shown:?} never closed");
}

/// Ticks until `cond` holds (at most `max` ticks).
fn until(host: &mut ProjectHost, max: u32, mut cond: impl FnMut(&ProjectHost) -> bool) -> u32 {
    for n in 0..max {
        if cond(host) {
            return n;
        }
        tick(host, &[]);
    }
    panic!("condition not met in {max} ticks");
}

fn var(host: &ProjectHost, name: &str) -> ScriptValue {
    host.scripts()
        .unwrap()
        .world()
        .get()
        .vars
        .get(name)
        .cloned()
        .unwrap_or(ScriptValue::Nil)
}

fn keeper(host: &ProjectHost) -> (GridPos, Direction) {
    let scripts = host.scripts().unwrap();
    let w = scripts.world().get();
    let npc = w.npc("keeper").unwrap();
    (npc.walker.pos(), npc.walker.facing())
}

#[test]
fn keeper_cutscene_moves_the_npc_asks_and_sets_the_flag() {
    let dir = project_dir("cutscene");
    // The player stands right below the keeper (5, 3).
    let mut host = host(&dir, GridPos::new(5, 4));
    assert_eq!(keeper(&host), (GridPos::new(5, 3), Direction::Down));
    assert_eq!(host.walker().facing(), Direction::Down);

    // Turn up (a tap turns without stepping), then talk.
    tick(&mut host, &[Key::Up]);
    idle(&mut host, 2);
    assert_eq!(host.walker().facing(), Direction::Up);
    assert_eq!(host.walker().pos(), GridPos::new(5, 4));
    keeper_turns_to(&host, Direction::Down);
    tick(&mut host, &[Key::Z]);
    assert_eq!(overlay(&host), say("text:keeper.hello"));
    assert!(
        host.scripts().unwrap().player_locked(),
        "interact locks the player"
    );

    close_text_box(&mut host);
    until(&mut host, 5, |h| overlay(h).is_some());
    assert_eq!(
        overlay(&host),
        Some(UiRequest::Choose {
            prompt: Some("text:keeper.help_me".into()),
            options: vec!["Yes".into(), "No".into()],
        })
    );
    assert_eq!(host.overlay().unwrap().cursor(), Some(0));
    tick(&mut host, &[Key::Z]); // "Yes"
    assert_eq!(overlay(&host), None);

    // The keeper walks up, up, left while the player stays locked.
    until(&mut host, 5, |h| {
        h.scripts()
            .unwrap()
            .world()
            .get()
            .npc("keeper")
            .unwrap()
            .path
            .is_some()
    });
    for _ in 0..10 {
        tick(&mut host, &[Key::Down]);
    }
    assert_eq!(
        host.walker().pos(),
        GridPos::new(5, 4),
        "locked player cannot walk"
    );
    let walked = until(&mut host, 200, |h| {
        let scripts = h.scripts().unwrap();
        let w = scripts.world().get();
        let npc = w.npc("keeper").unwrap();
        npc.path.is_none() && npc.walker.pos() == GridPos::new(4, 1)
    });
    assert!(walked > 0);
    assert_eq!(keeper(&host).1, Direction::Left);
    let entity = host
        .scene()
        .entities()
        .iter()
        .find(|e| e.id == 1)
        .copied()
        .unwrap();
    assert_eq!(
        entity.pos.at(1.0),
        (4.0 * 16.0, 16.0),
        "scene follows the npc"
    );

    // wait(20), then the thanks box.
    let waited = until(&mut host, 60, |h| overlay(h).is_some());
    assert!(waited >= 19, "wait(20) holds the cutscene ({waited} ticks)");
    assert_eq!(overlay(&host), say("text:keeper.thanks"));
    assert!(
        !host
            .scripts()
            .unwrap()
            .world()
            .get()
            .flags
            .contains_key("keeper_met")
    );
    close_text_box(&mut host);
    idle(&mut host, 2);

    let scripts = host.scripts().unwrap();
    {
        let w = scripts.world().get();
        assert_eq!(w.flags.get("keeper_met"), Some(&true));
        assert_eq!(w.bag.get("lamp_oil"), Some(&1));
    }
    assert!(scripts.scheduler().is_empty());
    assert!(!scripts.player_locked());

    // Unlocked again: the player can walk.
    for _ in 0..40 {
        tick(&mut host, &[Key::Down]);
    }
    assert_ne!(host.walker().pos(), GridPos::new(5, 4));
    std::fs::remove_dir_all(&dir).unwrap();
}

fn keeper_turns_to(host: &ProjectHost, dir: Direction) {
    assert_eq!(keeper(host).1, dir);
}

#[test]
fn declining_skips_the_walk_and_leaves_the_flag_unset() {
    let dir = project_dir("cutscene-no");
    let mut host = host(&dir, GridPos::new(5, 4));
    tick(&mut host, &[Key::Up]);
    idle(&mut host, 2);
    tick(&mut host, &[Key::Z]);
    close_text_box(&mut host);
    until(&mut host, 5, |h| overlay(h).is_some());
    tick(&mut host, &[Key::Down]);
    assert_eq!(host.overlay().unwrap().cursor(), Some(1));
    tick(&mut host, &[Key::Z]); // "No"
    idle(&mut host, 3);
    let scripts = host.scripts().unwrap();
    assert!(scripts.scheduler().is_empty());
    assert_eq!(keeper(&host).0, GridPos::new(5, 3));
    assert!(!scripts.world().get().flags.contains_key("keeper_met"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn engine_events_triggers_and_map_script_unload() {
    let dir = project_dir("cutscene-events");
    let mut host = host(&dir, GridPos::new(2, 5));
    // game_start first, then map_enter for every listener in registration order.
    assert_eq!(
        var(&host, "order"),
        ScriptValue::from("game:start;game:enter:lighthouse_1f;map:enter:lighthouse_1f;")
    );

    // Walk down two tiles onto the trigger (2, 7): two steps, one trigger_enter.
    let walk = |host: &mut ProjectHost, key: Key, target: GridPos| {
        for _ in 0..80 {
            if host.walker().pos() == target && host.walker().is_idle() {
                return;
            }
            tick(host, &[key]);
        }
        panic!("did not reach {target}");
    };
    walk(&mut host, Key::Down, GridPos::new(2, 7));
    idle(&mut host, 2);
    assert_eq!(var(&host, "steps"), ScriptValue::Int(2));
    let order = var(&host, "order");
    assert!(
        order
            .as_str()
            .unwrap()
            .ends_with("map:stairs_hint;map:trigger:stairs_hint;"),
        "on_enter runs right away; listeners in the events phase: {order:?}"
    );

    // `once`: stepping off and on again does not fire it twice.
    walk(&mut host, Key::Down, GridPos::new(2, 8));
    walk(&mut host, Key::Up, GridPos::new(2, 7));
    idle(&mut host, 2);
    let fired = var(&host, "order")
        .as_str()
        .unwrap()
        .matches("map:trigger:")
        .count();
    assert_eq!(fired, 1);

    // A scripted warp to the template's start map unloads this map's handlers.
    let map = ScriptOwner::Map("lighthouse_1f".into());
    host.scripts_mut()
        .unwrap()
        .host_mut()
        .run_source(&map, "warp.lua", "map.warp('start', 5, 7, 'down')")
        .unwrap();
    until(&mut host, 120, |h| h.scene().map().id == "start");
    idle(&mut host, 2);
    let order = var(&host, "order");
    assert!(
        order
            .as_str()
            .unwrap()
            .ends_with("game:exit:lighthouse_1f;map:exit:lighthouse_1f;game:enter:start;"),
        "map_exit reaches the old map's listeners, map_enter only the new ones: {order:?}"
    );
    let scripts = host.scripts_mut().unwrap();
    assert_eq!(scripts.map_owner(), Some(&ScriptOwner::Map("start".into())));
    assert!(!scripts.host_mut().has_function(&map, "old_keeper"));
    assert!(scripts.world().get().npcs.is_empty());
    std::fs::remove_dir_all(&dir).unwrap();
}
