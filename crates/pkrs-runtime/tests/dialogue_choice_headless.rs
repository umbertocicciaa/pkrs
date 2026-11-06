//! Headless stack integration: dialogue confirm pops overlay; choice menu uses Down + Confirm.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::Cell;
use std::rc::Rc;

use pkrs_platform::{FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig};
use pkrs_runtime::{
    ChoiceState, DialogueState, LoopExit, RuntimeHost, RuntimeLoopConfig, UiTheme, block_on_loop,
    build_test_ui_theme,
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
    let theme: UiTheme = build_test_ui_theme(host.renderer_mut()).unwrap();
    let dialogue = DialogueState::new(theme, host.renderer_mut(), "OK", 100, |k| {
        if k == "player" {
            "Player".into()
        } else {
            String::new()
        }
    });
    host.push_state(Box::new(dialogue));
    assert_eq!(host.stack().format_debug_stack(), "title > dialogue");

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
    let theme: UiTheme = build_test_ui_theme(host.renderer_mut()).unwrap();
    let result = Rc::new(Cell::new(None::<Option<usize>>));
    let result_for_state = Rc::clone(&result);
    let choice = ChoiceState::new(
        theme,
        host.renderer_mut(),
        vec![
            ChoiceOption { label: "a".into() },
            ChoiceOption { label: "b".into() },
            ChoiceOption { label: "c".into() },
        ],
        result_for_state,
    )
    .unwrap();
    host.push_state(Box::new(choice));

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
    assert_eq!(result.get(), Some(Some(1)));
}

#[test]
fn choice_needs_down_frame_to_select_second() {
    let mut host = RuntimeHost::with_title();
    let theme: UiTheme = build_test_ui_theme(host.renderer_mut()).unwrap();
    let result = Rc::new(Cell::new(None::<Option<usize>>));
    let choice = ChoiceState::new(
        theme,
        host.renderer_mut(),
        vec![
            ChoiceOption { label: "a".into() },
            ChoiceOption { label: "b".into() },
        ],
        Rc::clone(&result),
    )
    .unwrap();
    host.push_state(Box::new(choice));

    run_ticks(
        &mut host,
        vec![
            NullFrame::idle(),
            NullFrame::with_keys([Key::Enter]),
            NullFrame::idle(),
        ],
        3,
    );
    assert_eq!(result.get(), Some(Some(0)));
}

#[test]
fn choice_cancel_returns_none() {
    let mut host = RuntimeHost::with_title();
    let theme: UiTheme = build_test_ui_theme(host.renderer_mut()).unwrap();
    let result = Rc::new(Cell::new(None::<Option<usize>>));
    let choice = ChoiceState::new(
        theme,
        host.renderer_mut(),
        vec![ChoiceOption {
            label: "only".into(),
        }],
        Rc::clone(&result),
    )
    .unwrap();
    host.push_state(Box::new(choice));

    run_ticks(
        &mut host,
        vec![
            NullFrame::idle(),
            NullFrame::with_keys([Key::X]),
            NullFrame::idle(),
        ],
        3,
    );
    assert_eq!(result.get(), Some(None));
}
