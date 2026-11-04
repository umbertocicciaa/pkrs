//! Direction-aware clip lookup (`walk_<dir>`, horizontal mirroring).

use crate::animation::descriptor::{AnimationClip, SpriteSheetAnimation};
use crate::error::{RenderError, Result};

/// Resolved clip plus optional horizontal flip for drawing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DirectionalClip<'a> {
    /// Clip to play.
    pub clip: &'a AnimationClip,
    /// Mirror horizontally when blitting (e.g. reuse `walk_left` for right).
    pub flip_x: bool,
}

/// Resolves `prefix_<direction>` (e.g. `walk_down`) on `sheet`.
///
/// For `right`, falls back to `prefix_left` with [`DirectionalClip::flip_x`] when
/// `prefix_right` is absent (§5.4).
pub fn clip_for_direction<'a>(
    sheet: &'a SpriteSheetAnimation,
    prefix: &str,
    direction: &str,
) -> Result<DirectionalClip<'a>> {
    let primary = format!("{prefix}_{direction}");
    if let Some(clip) = sheet.clip(&primary) {
        return Ok(DirectionalClip {
            clip,
            flip_x: false,
        });
    }

    if direction == "right" {
        let mirror = format!("{prefix}_left");
        if let Some(clip) = sheet.clip(&mirror) {
            return Ok(DirectionalClip { clip, flip_x: true });
        }
    }

    Err(RenderError::AnimationClipNotFound { name: primary })
}
