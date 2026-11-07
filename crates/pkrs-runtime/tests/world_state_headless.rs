//! Integration (§6.2, §6.5, §9.2): NPCs, items, signs and triggers spawned from map objects,
//! interaction by facing + Confirm, and persistent map state that survives leaving and
//! re-entering the map: a moved NPC, a hidden NPC, a picked-up item, a fired `once` trigger
//! and map variables.

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
use pkrs_save::Value;
use pkrs_script::{ScriptHost, ScriptOwner};

const FIXTURE_FILES: &[&str] = &["maps/harbor.tmx", "scripts/maps/harbor.lua"];

fn project_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("pkrs-{tag}-{}-{nanos}", std::process::id()));
    let vars = TemplateVars::for_project("harbor").unwrap();
    scaffold(Template::OneShot, &dir, &vars).unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/world_state");
    for file in FIXTURE_FILES {
        let to = dir.join(file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(fixtures.join(file), &to).unwrap();
    }
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
        start_project_scripts(&project, Box::new(Pcg64Rng::from_seed(29))).unwrap();
    assert!(report.failed.is_empty());
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
        Box::new(FixedPaths::under("world-state")),
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

fn walk(host: &mut ProjectHost, key: Key, target: GridPos) {
    for _ in 0..120 {
        if host.walker().pos() == target && host.walker().is_idle() {
            return;
        }
        tick(host, &[key]);
    }
    panic!("did not reach {target} (at {})", host.walker().pos());
}

/// Taps `key` to turn without stepping.
fn turn(host: &mut ProjectHost, key: Key, dir: Direction) {
    tick(host, &[key]);
    idle(host, 2);
    assert_eq!(host.walker().facing(), dir);
}

fn overlay(host: &ProjectHost) -> Option<UiRequest> {
    host.overlay().map(|o| o.request().clone())
}

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

fn map_var(host: &ProjectHost, map: &str, name: &str) -> Option<Value> {
    let scripts = host.scripts().unwrap();
    let w = scripts.world().get();
    w.state.map(map).and_then(|m| m.var(name)).cloned()
}

fn npc(host: &ProjectHost, id: &str) -> (GridPos, Direction, bool) {
    let scripts = host.scripts().unwrap();
    let w = scripts.world().get();
    let npc = w.npc(id).unwrap();
    (npc.walker.pos(), npc.walker.facing(), npc.visible)
}

fn has_entity(host: &ProjectHost, id: u32) -> bool {
    host.scene().entities().iter().any(|e| e.id == id)
}

fn warp(host: &mut ProjectHost, map: &str, x: i32, y: i32, facing: &str) {
    host.scripts_mut()
        .unwrap()
        .host_mut()
        .run_source(
            &ScriptOwner::Game,
            "warp.lua",
            &format!("map.warp('{map}', {x}, {y}, '{facing}')"),
        )
        .unwrap();
    until(host, 120, |h| h.scene().map().id == map && !h.is_warping());
    idle(host, 2);
}

#[test]
fn interacting_then_leaving_and_re_entering_keeps_map_state() {
    let dir = project_dir("world-state");
    // The player stands right below the sailor (5, 3).
    let mut host = host(&dir, GridPos::new(5, 4));
    assert_eq!(map_var(&host, "harbor", "visits"), Some(Value::Int(1)));
    assert_eq!(
        npc(&host, "ghost"),
        (GridPos::new(8, 8), Direction::Up, true)
    );
    assert!(has_entity(&host, 3), "the item ball is spawned");

    // Talk to the sailor: facing + Confirm.
    turn(&mut host, Key::Up, Direction::Up);
    tick(&mut host, &[Key::Z]);
    assert_eq!(
        overlay(&host),
        Some(UiRequest::Say("text:sailor.hello".into()))
    );
    close_text_box(&mut host);
    until(&mut host, 200, |h| {
        let (pos, _, _) = npc(h, "sailor");
        pos == GridPos::new(4, 3) && !h.scripts().unwrap().player_locked()
    });
    assert_eq!(npc(&host, "sailor").1, Direction::Left);
    assert!(!npc(&host, "ghost").2, "the script hid the ghost");
    assert_eq!(map_var(&host, "harbor", "talks"), Some(Value::Int(1)));

    // Pick up the item ball at (2, 4): walk to (3, 4), it blocks the way, face it, Confirm.
    walk(&mut host, Key::Left, GridPos::new(3, 4));
    for _ in 0..20 {
        tick(&mut host, &[Key::Left]);
    }
    assert_eq!(
        host.walker().pos(),
        GridPos::new(3, 4),
        "item balls are solid"
    );
    assert_eq!(host.walker().facing(), Direction::Left);
    idle(&mut host, 2);
    tick(&mut host, &[Key::Z]);
    idle(&mut host, 2);
    {
        let scripts = host.scripts().unwrap();
        let w = scripts.world().get();
        assert_eq!(w.state.item_count("potion"), 1);
        assert_eq!(
            w.state.var("picked"),
            Some(&Value::from("potion:harbor_potion:1"))
        );
    }
    assert!(!has_entity(&host, 3), "the picked item is despawned");
    // Confirm again on the now empty tile does nothing.
    tick(&mut host, &[Key::Z]);
    idle(&mut host, 2);
    assert_eq!(
        host.scripts()
            .unwrap()
            .world()
            .get()
            .state
            .item_count("potion"),
        1
    );

    // Step on the once trigger at (2, 6).
    walk(&mut host, Key::Down, GridPos::new(3, 6));
    walk(&mut host, Key::Left, GridPos::new(2, 6));
    idle(&mut host, 3);
    assert_eq!(map_var(&host, "harbor", "hints"), Some(Value::Int(1)));

    // Leave for the template's start map: map vars are per map.
    warp(&mut host, "start", 5, 7, "down");
    assert_eq!(map_var(&host, "start", "talks"), None);
    assert!(
        host.scripts()
            .unwrap()
            .world()
            .get()
            .npc("sailor")
            .is_none()
    );

    // Come back.
    warp(&mut host, "harbor", 4, 4, "up");
    assert_eq!(map_var(&host, "harbor", "visits"), Some(Value::Int(2)));
    assert_eq!(
        npc(&host, "sailor"),
        (GridPos::new(4, 3), Direction::Left, true),
        "the sailor stays where the script moved him"
    );
    assert!(!npc(&host, "ghost").2, "the ghost stays hidden");
    assert!(!has_entity(&host, 2), "hidden NPCs have no scene entity");
    assert!(!has_entity(&host, 3), "the picked item stays gone");
    assert!(
        host.scripts()
            .unwrap()
            .world()
            .get()
            .item_at(GridPos::new(2, 4))
            .is_none()
    );
    assert!(
        host.scripts()
            .unwrap()
            .world()
            .get()
            .state
            .flag("sailor_met")
    );

    // The sailor remembers: map vars survived.
    tick(&mut host, &[Key::Z]);
    assert_eq!(
        overlay(&host),
        Some(UiRequest::Say("text:sailor.again".into()))
    );
    close_text_box(&mut host);
    idle(&mut host, 3);
    assert_eq!(map_var(&host, "harbor", "talks"), Some(Value::Int(2)));

    // The once trigger never fires again, not even after re-entering the map.
    walk(&mut host, Key::Down, GridPos::new(4, 6));
    walk(&mut host, Key::Left, GridPos::new(2, 6));
    walk(&mut host, Key::Right, GridPos::new(3, 6));
    walk(&mut host, Key::Left, GridPos::new(2, 6));
    idle(&mut host, 3);
    assert_eq!(map_var(&host, "harbor", "hints"), Some(Value::Int(1)));
    {
        let scripts = host.scripts().unwrap();
        let w = scripts.world().get();
        assert!(w.state.map("harbor").unwrap().has_fired("4"));
        assert_eq!(w.state.item_count("potion"), 1);
    }

    // Walking onto the old item tile works now that the ball is gone.
    walk(&mut host, Key::Up, GridPos::new(2, 4));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn signs_show_their_text_when_faced() {
    let dir = project_dir("world-state-sign");
    // Sign at (7, 4); stand on (7, 5) and face up.
    let mut host = host(&dir, GridPos::new(7, 5));
    turn(&mut host, Key::Up, Direction::Up);
    tick(&mut host, &[Key::Z]);
    idle(&mut host, 1);
    assert_eq!(
        overlay(&host),
        Some(UiRequest::Say("text:harbor.board".into()))
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
