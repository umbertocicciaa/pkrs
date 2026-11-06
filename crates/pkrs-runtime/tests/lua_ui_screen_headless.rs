//! Headless integration: `ui.push_screen` opens a [`LuaUiState`]; Cancel pops and wakes the task (issue #47).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_core::Pcg64Rng;
use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::{LoopExit, RuntimeHost, RuntimeLoopConfig, block_on_loop};
use pkrs_script::{
    LuaScriptHost, MemoryScripts, Scheduler, ScriptHost, ScriptLimits, ScriptOwner, TaskKind,
};

#[test]
fn lua_push_screen_pops_on_cancel_and_resumes_task() {
    let files = MemoryScripts::new();
    let mut lua = LuaScriptHost::new(
        Box::new(files),
        Box::new(Pcg64Rng::from_seed(47)),
        ScriptLimits::default(),
    )
    .unwrap();

    let init = r#"
ui.register_screen("test_modal", {
  on_enter = function() end,
  update = function(self, input)
    if input.pressed("cancel") then
      return "pop"
    end
  end,
  render = function(self, gfx)
    gfx.window(8, 8, 100, 50)
  end,
})
function push_modal()
  return ui.push_screen("test_modal")
end
"#;
    lua.run_source(&ScriptOwner::Game, "scripts/ui.lua", init)
        .unwrap();

    let scheduler = Scheduler::new();
    let mut host = RuntimeHost::with_title_and_scripts(lua, scheduler);

    {
        let ctx = host.runtime_ctx();
        let lua = ctx.script_host.as_mut().unwrap();
        let scheduler = ctx.scheduler.as_mut().unwrap();
        scheduler
            .spawn(
                lua,
                &ScriptOwner::Game,
                "push_modal",
                &[],
                TaskKind::Background,
            )
            .unwrap();
    }

    let mut frames: Vec<NullFrame> = std::iter::repeat_n(NullFrame::idle(), 40).collect();
    // Second simulated frame: press Cancel (`Key::X`) before the tick that reads input.
    frames[1] = NullFrame::with_keys([Key::X]);

    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(frames)),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );

    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(25),
            ..RuntimeLoopConfig::default()
        },
    );

    assert_eq!(exit, LoopExit::TickLimit);
    assert_eq!(host.stack().len(), 1);
    assert_eq!(host.stack().top_id(), Some("title"));

    let ctx = host.runtime_ctx();
    let scheduler = ctx.scheduler.as_ref().unwrap();
    assert!(
        scheduler.is_empty(),
        "task should finish after UI pop; still have {} tasks",
        scheduler.len()
    );
}
