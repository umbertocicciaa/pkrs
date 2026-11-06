//! Placeholder title screen until assets and menu flow land.

use pkrs_render::Renderer;

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;

/// Built-in title state (§4.1); pushes gameplay via script or menu later.
#[derive(Clone, Copy, Debug, Default)]
pub struct TitleState;

impl GameState for TitleState {
    fn id(&self) -> &str {
        "title"
    }

    fn update(&mut self, _ctx: &mut RuntimeCtx) -> Transition {
        Transition::None
    }

    fn render(&self, _renderer: &mut Renderer, _alpha: f32) {}
}
