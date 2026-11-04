//! Tick-based clip playback (simulation clock, not frame time).

use crate::animation::descriptor::AnimationClip;

/// Advances one clip on simulation ticks (§3.1, §5.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipPlayer {
    frames: Vec<u32>,
    ticks_per_frame: u32,
    r#loop: bool,
    frame_pos: usize,
    tick_in_frame: u32,
    finished: bool,
}

impl ClipPlayer {
    /// Starts at the first frame of `clip`.
    pub fn new(clip: &AnimationClip) -> Self {
        Self {
            frames: clip.frames.clone(),
            ticks_per_frame: clip.ticks_per_frame,
            r#loop: clip.r#loop,
            frame_pos: 0,
            tick_in_frame: 0,
            finished: false,
        }
    }

    /// Whether a non-looping clip has reached and held its last frame.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Index into the sheet grid for the frame currently displayed.
    pub fn current_frame_index(&self) -> u32 {
        self.frames[self.frame_pos]
    }

    /// Zero-based position within the clip's `frames` list.
    pub fn current_frame_pos(&self) -> usize {
        self.frame_pos
    }

    /// Advances one simulation tick; no-op when a non-looping clip is finished.
    pub fn tick(&mut self) {
        if self.finished {
            return;
        }
        self.tick_in_frame = self.tick_in_frame.saturating_add(1);
        if self.tick_in_frame < self.ticks_per_frame {
            return;
        }
        self.tick_in_frame = 0;
        if self.frame_pos + 1 < self.frames.len() {
            self.frame_pos += 1;
            return;
        }
        if self.r#loop {
            self.frame_pos = 0;
        } else {
            self.finished = true;
        }
    }

    /// Resets to the first frame without changing clip data.
    pub fn reset(&mut self) {
        self.frame_pos = 0;
        self.tick_in_frame = 0;
        self.finished = false;
    }
}

/// Returns the frame list index after `ticks` simulation steps from the start of `clip`.
///
/// Useful for table-driven unit tests of timing.
pub fn frame_pos_after_ticks(clip: &AnimationClip, ticks: u64) -> usize {
    let mut player = ClipPlayer::new(clip);
    for _ in 0..ticks {
        player.tick();
    }
    player.current_frame_pos()
}
