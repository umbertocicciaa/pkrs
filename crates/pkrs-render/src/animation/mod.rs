//! Sprite sheet animation: RON descriptors and tick-based clip playback (§5.4).

mod descriptor;
mod direction;
mod player;

pub use descriptor::{AnimationClip, SpriteSheetAnimation, parse_animation_descriptor};
pub use direction::{DirectionalClip, clip_for_direction};
pub use player::ClipPlayer;
