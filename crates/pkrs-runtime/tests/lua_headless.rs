//! Integration (§9.6): load a fixture `scripts/main.lua` through the VFS and run it from the
//! headless runtime loop, one Lua call per simulation tick.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;

use pkrs_assets::FolderVfs;
use pkrs_core::Pcg64Rng;
use pkrs_platform::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::scripts::VfsScriptFiles;
use pkrs_runtime::{LoopExit, RuntimeLoopConfig, SimulationHost, block_on_loop};
use pkrs_script::{LuaScriptHost, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue};

fn lua_host(seed: u64) -> LuaScriptHost {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/lua_project");
    let files = VfsScriptFiles::new(FolderVfs::new(root).unwrap());
    LuaScriptHost::new(
        Box::new(files),
        Box::new(Pcg64Rng::from_seed(seed)),
        ScriptLimits::default(),
    )
    .unwrap()
}

/// Calls the game's `tick()` on every simulation tick; script errors are counted, not fatal.
struct ScriptedHost {
    lua: LuaScriptHost,
    errors: u32,
}

impl SimulationHost for ScriptedHost {
    fn poll_input(&mut self, _: &Platform) {}
    fn begin_tick(&mut self) {}
    fn simulation_tick(&mut self, _dt: f64) {
        if self
            .lua
            .call_function(&ScriptOwner::Game, "tick", &[])
            .is_err()
        {
            self.errors += 1;
        }
    }
    fn render(&mut self, _: &Platform, _: f32) {}
}

fn run(seed: u64, ticks: u64) -> ScriptValue {
    let mut lua = lua_host(seed);
    lua.run_file(&ScriptOwner::Game, "scripts/main.lua")
        .unwrap();
    let mut host = ScriptedHost { lua, errors: 0 };
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
    assert_eq!(host.errors, 0);
    host.lua
        .call_function(&ScriptOwner::Game, "summary", &[])
        .unwrap()
}

#[test]
fn main_lua_runs_every_tick_headless() {
    let ScriptValue::Map(summary) = run(7, 60) else {
        panic!("summary must be a table")
    };
    assert_eq!(summary["ticks"], ScriptValue::Int(60));
    assert_eq!(summary["sandboxed"], ScriptValue::Bool(true));
    let total = summary["total"].as_int().unwrap();
    assert!((60..=360).contains(&total), "60 rolls of 1..=6: {total}");
}

#[test]
fn script_rolls_are_deterministic_for_a_seed() {
    assert_eq!(run(99, 30), run(99, 30));
}

#[test]
fn fixture_cannot_escape_the_sandbox() {
    let mut lua = lua_host(1);
    assert_eq!(
        lua.run_file(&ScriptOwner::Game, "scripts/escape.lua")
            .unwrap(),
        ScriptValue::Int(0),
        "no escape attempt may succeed"
    );
}
