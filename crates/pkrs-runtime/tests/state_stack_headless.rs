//! Headless integration: scripted stack transitions and lifecycle trace.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{FixedPaths, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::{
    FadeTransition, GameState, LoopExit, RuntimeCtx, RuntimeHost, RuntimeLoopConfig, Transition,
    block_on_loop,
};
use std::cell::RefCell;
use std::rc::Rc;

struct ScriptedDriver {
    log: Rc<RefCell<Vec<String>>>,
    updates_while_top: u32,
}

impl GameState for ScriptedDriver {
    fn id(&self) -> &'static str {
        "script"
    }

    fn on_enter(&mut self, _: &mut RuntimeCtx) {
        self.log.borrow_mut().push("enter:script".into());
    }

    fn on_exit(&mut self, _: &mut RuntimeCtx) {
        self.log.borrow_mut().push("exit:script".into());
    }

    fn on_resume(&mut self, _: &mut RuntimeCtx) {
        self.log.borrow_mut().push("resume:script".into());
    }

    fn update(&mut self, _: &mut RuntimeCtx) -> Transition {
        self.updates_while_top += 1;
        match self.updates_while_top {
            1 => Transition::Push(Box::new(FadeTransition::new(1))),
            2 => Transition::Quit,
            _ => Transition::None,
        }
    }

    fn render(&self, _: &mut pkrs_render::Renderer, _: f32) {}
}

#[test]
fn headless_run_applies_scripted_transitions_after_update() {
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut host = RuntimeHost::with_title();
    host.push_state(Box::new(ScriptedDriver {
        log: log.clone(),
        updates_while_top: 0,
    }));

    let mut platform = Platform::new(
        Box::new(NullPlatform::new()),
        Box::new(FixedPaths::under("test")),
        PlatformConfig::default(),
    );

    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(6),
            ..RuntimeLoopConfig::default()
        },
    );

    assert_eq!(exit, LoopExit::TickLimit);
    assert!(host.should_quit());

    let events = log.borrow();
    assert_eq!(events.as_slice(), &["enter:script", "resume:script"]);
    assert_eq!(host.stack().format_debug_stack(), "title > script");
}
