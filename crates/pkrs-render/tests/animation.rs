//! Unit tests for sprite animation descriptors and clip playback.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use pkrs_core::Direction;
use pkrs_render::{
    AnimationClip, ClipPlayer, SpriteSheetAnimation, clip_for_direction, parse_animation_descriptor,
};

const WALK_SHEET: &str = r#"(
    frame_size: (16, 16),
    columns: 4,
    clips: {
        "walk_down": (
            frames: [0, 1, 2, 3],
            ticks_per_frame: 4,
            loop: true,
        ),
        "walk_left": (
            frames: [4, 5],
            ticks_per_frame: 2,
            loop: true,
        ),
        "idle": (
            frames: [0],
            ticks_per_frame: 60,
            loop: false,
        ),
    },
)"#;

fn sample_sheet() -> SpriteSheetAnimation {
    parse_animation_descriptor(WALK_SHEET).expect("fixture parses")
}

fn frame_pos_after_ticks(clip: &AnimationClip, ticks: u64) -> usize {
    let mut player = ClipPlayer::new(clip);
    for _ in 0..ticks {
        player.tick();
    }
    player.current_frame_pos()
}

#[test]
fn parse_valid_descriptor() {
    let sheet = sample_sheet();
    assert_eq!(sheet.frame_width, 16);
    assert_eq!(sheet.columns, 4);
    assert_eq!(sheet.clips.len(), 3);
}

#[test]
fn parse_errors_on_invalid_ron() {
    assert!(parse_animation_descriptor("(not_a_struct)").is_err());
}

#[test]
fn parse_errors_on_empty_frames() {
    let src = r#"(
        frame_size: (8, 8),
        columns: 1,
        clips: {
            "bad": (frames: [], ticks_per_frame: 1, loop: true),
        },
    )"#;
    assert!(parse_animation_descriptor(src).is_err());
}

#[test]
fn parse_errors_on_zero_ticks_per_frame() {
    let src = r#"(
        frame_size: (8, 8),
        columns: 1,
        clips: {
            "bad": (frames: [0], ticks_per_frame: 0, loop: true),
        },
    )"#;
    assert!(parse_animation_descriptor(src).is_err());
}

#[test]
fn parse_errors_on_frame_address_overflow() {
    let src = r#"(
        frame_size: (16, 16),
        columns: 4,
        clips: {
            "bad": (frames: [4000000000], ticks_per_frame: 1, loop: true),
        },
    )"#;
    assert!(parse_animation_descriptor(src).is_err());
}

#[test]
fn frame_src_rect_steps_horizontally() {
    let sheet = sample_sheet();
    assert_eq!(sheet.frame_src_rect(0).x, 0.0);
    assert_eq!(sheet.frame_src_rect(1).x, 16.0);
    assert_eq!(sheet.frame_src_rect(4).y, 16.0);
}

#[test]
fn clip_player_loops_with_correct_frame_timing() {
    let sheet = sample_sheet();
    let clip = sheet.clip("walk_down").expect("clip");
    assert_eq!(frame_pos_after_ticks(clip, 0), 0);
    assert_eq!(frame_pos_after_ticks(clip, 3), 0);
    assert_eq!(frame_pos_after_ticks(clip, 4), 1);
    assert_eq!(frame_pos_after_ticks(clip, 15), 3);
    assert_eq!(frame_pos_after_ticks(clip, 16), 0);
}

#[test]
fn clip_player_non_loop_holds_last_frame() {
    let src = r#"(
        frame_size: (16, 16),
        columns: 4,
        clips: {
            "dash": (
                frames: [0, 1, 2],
                ticks_per_frame: 2,
                loop: false,
            ),
        },
    )"#;
    let sheet = parse_animation_descriptor(src).expect("parse");
    let clip = sheet.clip("dash").expect("clip");
    let mut player = ClipPlayer::new(clip);
    for _ in 0..5 {
        player.tick();
    }
    assert!(!player.is_finished());
    assert_eq!(player.current_frame_index(), 2);
    player.tick();
    assert!(player.is_finished());
    assert_eq!(player.current_frame_index(), 2);
    for _ in 0..100 {
        player.tick();
    }
    assert!(player.is_finished());
    assert_eq!(player.current_frame_index(), 2);
}

#[test]
fn clip_player_empty_frames_does_not_panic() {
    let clip = AnimationClip {
        frames: vec![],
        ticks_per_frame: 1,
        r#loop: true,
    };
    let mut player = ClipPlayer::new(&clip);
    assert_eq!(player.current_frame_index(), 0);
    for _ in 0..10 {
        player.tick();
    }
    assert_eq!(player.current_frame_index(), 0);
}

#[test]
fn direction_clip_uses_mirror_for_right() {
    let sheet = sample_sheet();
    let resolved = clip_for_direction(&sheet, "walk", Direction::Right).expect("mirror");
    assert!(resolved.flip_x);
    assert_eq!(resolved.clip.frames, [4, 5]);
}

#[test]
fn direction_clip_prefers_explicit_right() {
    let mut sheet = sample_sheet();
    sheet.clips.insert(
        "walk_right".into(),
        AnimationClip {
            frames: vec![6, 7],
            ticks_per_frame: 2,
            r#loop: true,
        },
    );
    let resolved = clip_for_direction(&sheet, "walk", Direction::Right).expect("explicit");
    assert!(!resolved.flip_x);
    assert_eq!(resolved.clip.frames, [6, 7]);
}

#[test]
fn direction_clip_missing_returns_error() {
    let sheet = sample_sheet();
    assert!(clip_for_direction(&sheet, "walk", Direction::Up).is_err());
}
