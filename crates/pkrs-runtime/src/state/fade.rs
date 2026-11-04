//! Full-screen fade transition (§6.4 warps); pops itself when complete.

use pkrs_render::{Layer, Rect, Renderer, SpriteDraw, TextureHandle};

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
        if self.ticks_remaining == 0 {
            Transition::Pop
        } else {
            self.ticks_remaining -= 1;
            Transition::None
        }
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        let display = renderer.display();
        renderer.draw_sprite(SpriteDraw {
            texture: TextureHandle(0),
            src: Rect::new(0.0, 0.0, 1.0, 1.0),
            dst: Rect::new(0.0, 0.0, display.width as f32, display.height as f32),
            flip_x: false,
            layer: Layer::Transition,
        });
    }
}
