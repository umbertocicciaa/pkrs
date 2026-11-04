//! Integration: scripted [`NullPlatform`] keys through the main loop and [`Input`] facade.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{
    Action, BuiltinAction, FixedPaths, Input, Key, NullFrame, NullPlatform, Platform,
    PlatformConfig,
};
use pkrs_runtime::{LoopExit, RuntimeLoopConfig, SimulationHost, block_on_loop};

/// Minimal state machine for headless input tests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TestState {
    Overworld,
    Dialogue,
}

struct InputTestHost {
    input: Input,
    state: TestState,
    log: Vec<(u64, TestState, Action)>,
    tick: u64,
}

impl InputTestHost {
    fn new(input: Input) -> Self {
        Self {
            input,
            state: TestState::Overworld,
            log: Vec::new(),
            tick: 0,
        }
    }

    fn update_state(&mut self) {
        self.tick = self.tick.saturating_add(1);
        let confirm = Action::Builtin(BuiltinAction::Confirm);
        if self.input.triggered(confirm) {
            self.log.push((self.tick, self.state, confirm));
            match self.state {
                TestState::Overworld => {
                    self.state = TestState::Dialogue;
                }
                TestState::Dialogue => {
                    self.input.consume(confirm);
                }
            }
        }
    }
}

impl SimulationHost for InputTestHost {
    fn poll_input(&mut self, platform: &Platform) {
        self.input.poll(platform);
    }

    fn begin_tick(&mut self) {
        self.input.begin_tick();
    }

    fn simulation_tick(&mut self, _: f64) {
        self.update_state();
    }

    fn render(&mut self, _: &Platform, _: f32) {}
}

#[test]
fn scripted_confirm_reaches_dialogue_and_buffers_across_transition() {
    let frames = vec![
        NullFrame::idle(),
        NullFrame::with_keys([Key::Z]),
        NullFrame::with_keys([Key::Z]),
        NullFrame::idle(),
    ];
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(frames)),
        Box::new(FixedPaths::under("input-integration")),
        PlatformConfig::default(),
    );
    let mut host = InputTestHost::new(Input::default());
    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(4),
            max_frames: Some(4),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::TickLimit);
    assert_eq!(host.state, TestState::Dialogue);
    assert_eq!(
        host.log,
        vec![
            (
                3,
                TestState::Overworld,
                Action::Builtin(BuiltinAction::Confirm)
            ),
            (
                4,
                TestState::Dialogue,
                Action::Builtin(BuiltinAction::Confirm)
            ),
        ]
    );
}

#[test]
fn multi_tick_frame_emits_one_press_per_key_down() {
    struct PressCounter {
        input: Input,
        presses: u32,
    }
    impl SimulationHost for PressCounter {
        fn poll_input(&mut self, platform: &Platform) {
            self.input.poll(platform);
        }
        fn begin_tick(&mut self) {
            self.input.begin_tick();
        }
        fn simulation_tick(&mut self, _: f64) {
            if self.input.pressed(Action::Builtin(BuiltinAction::Confirm)) {
                self.presses += 1;
            }
        }
        fn render(&mut self, _: &Platform, _: f32) {}
    }

    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames([NullFrame {
            dt: 0.25,
            keys: vec![Key::Z],
            ..NullFrame::idle()
        }])),
        Box::new(FixedPaths::under("input-integration")),
        PlatformConfig::default(),
    );
    let mut host = PressCounter {
        input: Input::default(),
        presses: 0,
    };
    let exit = block_on_loop(
        &mut platform,
        &mut host,
        &RuntimeLoopConfig {
            max_ticks: Some(16),
            max_frames: Some(2),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::TickLimit);
    assert_eq!(host.presses, 1);
}
