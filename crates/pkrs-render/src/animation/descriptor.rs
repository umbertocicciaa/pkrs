//! RON sprite-sheet animation descriptors (§5.4).

use std::collections::BTreeMap;

use serde::Deserialize;

use crate::error::{RenderError, Result};
use crate::rect::Rect;

/// Parsed animation descriptor for one sprite sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteSheetAnimation {
    /// Frame width in texels.
    pub frame_width: u32,
    /// Frame height in texels.
    pub frame_height: u32,
    /// Number of frames per row in the sheet (left-to-right, then next row).
    pub columns: u32,
    /// Named clips keyed by clip id (e.g. `walk_down`).
    pub clips: BTreeMap<String, AnimationClip>,
}

/// One named animation clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnimationClip {
    /// Frame indices into the sheet grid (row-major using [`SpriteSheetAnimation::columns`]).
    pub frames: Vec<u32>,
    /// Simulation ticks each frame is held (60 Hz logic clock, §3.1).
    pub ticks_per_frame: u32,
    /// When true, the clip wraps to the first frame after the last.
    pub r#loop: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSheet {
    frame_size: (u32, u32),
    columns: u32,
    clips: BTreeMap<String, RawClip>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawClip {
    frames: Vec<u32>,
    ticks_per_frame: u32,
    #[serde(default = "default_loop")]
    r#loop: bool,
}

fn default_loop() -> bool {
    true
}

/// Parses a sprite animation descriptor from RON text.
pub fn parse_animation_descriptor(source: &str) -> Result<SpriteSheetAnimation> {
    let raw: RawSheet = ron::from_str(source).map_err(|e| RenderError::AnimationParse {
        detail: e.to_string(),
    })?;
    validate_and_convert(raw)
}

fn validate_and_convert(raw: RawSheet) -> Result<SpriteSheetAnimation> {
    if raw.frame_size.0 == 0 || raw.frame_size.1 == 0 {
        return Err(RenderError::AnimationInvalid {
            detail: "frame_size must be non-zero".into(),
        });
    }
    if raw.columns == 0 {
        return Err(RenderError::AnimationInvalid {
            detail: "columns must be at least 1".into(),
        });
    }
    if raw.clips.is_empty() {
        return Err(RenderError::AnimationInvalid {
            detail: "clips must not be empty".into(),
        });
    }

    let mut clips = BTreeMap::new();
    for (name, clip) in raw.clips {
        if clip.frames.is_empty() {
            return Err(RenderError::AnimationInvalid {
                detail: format!("clip `{name}` has no frames"),
            });
        }
        if clip.ticks_per_frame == 0 {
            return Err(RenderError::AnimationInvalid {
                detail: format!("clip `{name}` ticks_per_frame must be at least 1"),
            });
        }
        for &f in &clip.frames {
            let col = f % raw.columns;
            let row = f / raw.columns;
            let fits = col
                .checked_add(1)
                .and_then(|c| c.checked_mul(raw.frame_size.0))
                .zip(
                    row.checked_add(1)
                        .and_then(|r| r.checked_mul(raw.frame_size.1)),
                )
                .is_some();
            if !fits {
                return Err(RenderError::AnimationInvalid {
                    detail: format!("clip `{name}` frame {f} is outside the addressable sheet"),
                });
            }
        }
        clips.insert(
            name,
            AnimationClip {
                frames: clip.frames,
                ticks_per_frame: clip.ticks_per_frame,
                r#loop: clip.r#loop,
            },
        );
    }

    Ok(SpriteSheetAnimation {
        frame_width: raw.frame_size.0,
        frame_height: raw.frame_size.1,
        columns: raw.columns,
        clips,
    })
}

impl SpriteSheetAnimation {
    /// Source rectangle in texels for `frame_index` in the sheet grid.
    pub fn frame_src_rect(&self, frame_index: u32) -> Rect {
        let col = frame_index % self.columns;
        let row = frame_index / self.columns;
        let x = (col * self.frame_width) as f32;
        let y = (row * self.frame_height) as f32;
        Rect::new(x, y, self.frame_width as f32, self.frame_height as f32)
    }

    /// Looks up a clip by exact name.
    pub fn clip(&self, name: &str) -> Option<&AnimationClip> {
        self.clips.get(name)
    }
}
