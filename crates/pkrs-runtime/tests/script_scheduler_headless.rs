//! Integration (§9.3, §3.3 step 2): the headless runtime loop steps the coroutine scheduler in
//! the scripts phase of every tick; a cutscene waits, emits, and its tick numbers are exact.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use pkrs_assets::FolderVfs;
use pkrs_core::{Direction, Pcg64Rng};
use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::scripts::VfsScriptFiles;
use pkrs_runtime::{
    LoopExit, RuntimeLoopConfig, SimulationHost, TickHooks, block_on_loop, run_tick,
};
use pkrs_script::{
    LuaScriptHost, Scheduler, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue, TaskKind,
};

/// Spawns scripted tasks at fixed ticks (as the event phase would on `interact`) and steps the
/// scheduler in the scripts phase.
struct CutsceneHost {
    lua: LuaScriptHost,
    scheduler: Scheduler,
    /// `(tick, function, kind)` to start during that tick's event phase.
    spawns: Vec<(u64, &'static str, TaskKind)>,
    /// Player lock seen by the state-stack phase, per tick (index 0 = tick 1).
    locked: Vec<bool>,
    /// Gated "hold up" input seen by player movement, per tick.
    moves: Vec<Option<Direction>>,
    failures: usize,
}

impl TickHooks for CutsceneHost {
    fn scripts(&mut self, _dt: f64) {
        self.failures += self.scheduler.step(&mut self.lua).failures();
    }

    fn state_stack(&mut self, _dt: f64) {
        self.locked.push(self.scheduler.player_locked());
        self.moves
            .push(self.scheduler.player_lock().gate(Some(Direction::Up)));
    }

    fn events(&mut self) {
        let tick = self.scheduler.tick();
        let due: Vec<_> = self
            .spawns
            .iter()
            .filter(|s| s.0 == tick)
            .copied()
            .collect();
        for (_, name, kind) in due {
            let args = [ScriptValue::from("keeper")];
            if self
                .scheduler
                .spawn(&mut self.lua, &ScriptOwner::Game, name, &args, kind)
                .is_err()
            {
                self.failures += 1;
            }
        }
    }
}

impl SimulationHost for CutsceneHost {
    fn poll_input(&mut self, _: &Platform) {}
    fn begin_tick(&mut self) {}
    fn simulation_tick(&mut self, dt: f64) {
        run_tick(self, dt);
    }
    fn render(&mut self, _: &Platform, _: f32) {}
}

fn run(spawns: Vec<(u64, &'static str, TaskKind)>, ticks: u64) -> CutsceneHost {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lua_project");
    let mut lua = LuaScriptHost::new(
        Box::new(VfsScriptFiles::new(FolderVfs::new(root).unwrap())),
        Box::new(Pcg64Rng::from_seed(3)),
        ScriptLimits::default(),
    )
    .unwrap();
    lua.run_file(&ScriptOwner::Game, "scripts/cutscene.lua")
        .unwrap();
    let mut host = CutsceneHost {
        lua,
        scheduler: Scheduler::new(),
        spawns,
        locked: Vec::new(),
        moves: Vec::new(),
        failures: 0,
    };
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(std::iter::repeat_n(NullFrame::idle(), 1000))),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );
    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(ticks),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::TickLimit);
    assert_eq!(host.locked.len() as u64, ticks);
    host
}

fn emitted(host: &mut CutsceneHost) -> String {
    host.lua
        .call_function(&ScriptOwner::Game, "emitted_log", &[])
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn cutscene_waits_exact_ticks_and_locks_the_player() {
    let mut host = run(vec![(1, "on_interact", TaskKind::Interact)], 60);
    // Started in tick 1's event phase; wait(30) resumes at tick 31, wait(10) at tick 41.
    assert_eq!(emitted(&mut host), "start:keeper@1,after_wait@31,done@41");
    assert_eq!(host.failures, 0);
    for (i, locked) in host.locked.iter().enumerate() {
        let tick = i + 1;
        assert_eq!(
            *locked,
            (2..=40).contains(&tick),
            "lock state at tick {tick}"
        );
    }
    assert_eq!(host.moves[9], None, "movement is gated while locked");
    assert_eq!(host.moves[45], Some(Direction::Up));
    assert!(host.scheduler.is_empty());
}

#[test]
fn concurrent_tasks_interleave_in_creation_order() {
    // Both start in tick 10's event phase, background first: when both resume in the same tick
    // (50), the older task runs first.
    let spawns = vec![
        (10, "wander", TaskKind::Background),
        (10, "on_interact", TaskKind::Interact),
    ];
    let mut a = run(spawns.clone(), 70);
    let log = emitted(&mut a);
    assert_eq!(
        log,
        "start:keeper@10,wander@30,after_wait@40,wander@50,done@50,wander@70"
    );
    let mut b = run(spawns, 70);
    assert_eq!(log, emitted(&mut b), "same inputs, same order");
}

#[test]
fn failing_cutscene_unlocks_the_player_and_the_loop_keeps_running() {
    let mut host = run(
        vec![
            (1, "broken_cutscene", TaskKind::Trigger),
            (2, "wander", TaskKind::Background),
        ],
        70,
    );
    assert_eq!(host.failures, 1);
    assert_eq!(
        emitted(&mut host),
        "broken_start@1,wander@22,wander@42,wander@62"
    );
    // Locked from tick 2 until the error at tick 6.
    for (i, locked) in host.locked.iter().enumerate() {
        let tick = i + 1;
        assert_eq!(
            *locked,
            (2..=5).contains(&tick),
            "lock state at tick {tick}"
        );
    }
}
