//! Full-screen fade transition (§6.4 warps); pops itself when complete.

use pkrs_render::Renderer;

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;

/// Screen fade overlay; non-opaque so the map below keeps drawing during the effect.
#[derive(Clone, Copy, Debug)]
pub struct FadeTransition {
    ticks_remaining: u32,
}

impl FadeTransition {
    /// Fade lasting `ticks` simulation steps at 60 Hz.
    #[must_use]
    pub fn new(ticks: u32) -> Self {
        Self {
            ticks_remaining: ticks,
        }
    }
}

impl GameState for FadeTransition {
    fn id(&self) -> &'static str {
        "transition"
    }

    fn is_opaque(&self) -> bool {
        false
    }

    fn update(&mut self, _ctx: &mut RuntimeCtx) -> Transition {
        self.ticks_remaining = self.ticks_remaining.saturating_sub(1);
        if self.ticks_remaining == 0 {
            Transition::Pop
        } else {
            Transition::None
        }
    }

    fn render(&self, _renderer: &mut Renderer, _alpha: f32) {
        // Visual fade waits for a fill/tint draw primitive on `Renderer` (§4.5).
    }
}
