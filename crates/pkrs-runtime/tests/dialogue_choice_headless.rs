//! Headless stack integration: dialogue confirm pops overlay; choice menu uses Down + Confirm.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::{
    ChoiceState, DialogueState, LoopExit, RuntimeHost, RuntimeLoopConfig, block_on_loop,
};
use pkrs_ui::ChoiceOption;

fn run_ticks(host: &mut RuntimeHost, frames: Vec<NullFrame>, max_ticks: u64) {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new().with_frames(frames)),
        Box::new(FixedPaths::under("dialogue-choice-headless")),
        PlatformConfig::default(),
    );
    let exit = block_on_loop(
        &mut platform,
        host,
        &RuntimeLoopConfig {
            max_ticks: Some(max_ticks),
            max_frames: Some(max_ticks.saturating_add(2)),
            ..RuntimeLoopConfig::default()
        },
    );
    assert_eq!(exit, LoopExit::TickLimit, "expected tick limit, not quit");
}

#[test]
fn dialogue_confirm_pops_overlay() {
    let mut host = RuntimeHost::with_title();
    // Single page, fast typewriter: first tick reveals all; second tick confirm → Done → Pop.
    let dialogue = DialogueState::new(host.renderer_mut(), "OK", 100).unwrap();
    host.push_state(Box::new(dialogue));
    assert_eq!(host.stack().format_debug_stack(), "title > dialogue");

    // Frame 0: built-in idle. Frames 1–3: idle, Z held, idle → confirm on sim tick 3 pops overlay.
    run_ticks(
        &mut host,
        vec![
            NullFrame::idle(),
            NullFrame::with_keys([Key::Z]),
            NullFrame::idle(),
        ],
        3,
    );
    assert_eq!(host.stack().format_debug_stack(), "title");
}

#[test]
fn choice_down_and_confirm_pops_with_second_option() {
    let mut host = RuntimeHost::with_title();
    let choice = ChoiceState::new(
        host.renderer_mut(),
        vec![
            ChoiceOption { label: "a".into() },
            ChoiceOption { label: "b".into() },
            ChoiceOption { label: "c".into() },
        ],
    )
    .unwrap();
    host.push_state(Box::new(choice));

    // Built-in idle, scripted idle, Down, Enter, idle → Down on tick 2, Confirm on tick 4 → Pop.
    run_ticks(
        &mut host,
        vec![
            NullFrame::idle(),
            NullFrame::with_keys([Key::Down]),
            NullFrame::with_keys([Key::Enter]),
            NullFrame::idle(),
        ],
        4,
    );
    assert_eq!(host.stack().format_debug_stack(), "title");
}
