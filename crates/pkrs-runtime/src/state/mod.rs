//! Game state stack (§4.1): modal screens, deferred transitions, opaque rendering.

mod battle;
mod choice_state;
mod dialogue;
mod fade;
mod lua_ui;
mod save_screen;
mod stack;
mod title;

pub use battle::BattleState;
pub use choice_state::ChoiceState;
pub use dialogue::DialogueState;
pub use fade::FadeTransition;
pub use lua_ui::LuaUiState;
pub use stack::StateStack;
pub use title::TitleState;

use pkrs_render::Renderer;

use crate::ctx::RuntimeCtx;

/// Lifecycle hooks and per-tick logic for one screen on the stack.
pub trait GameState {
    /// Stable id used by [`Transition::PopTo`] and debug overlays.
    fn id(&self) -> &str;

    /// Called once when this state becomes the top of the stack.
    fn on_enter(&mut self, _ctx: &mut RuntimeCtx) {}

    /// Called once when this state is removed from the stack.
    fn on_exit(&mut self, _ctx: &mut RuntimeCtx) {}

    /// Called when a state above this one was popped and this state is top again.
    fn on_resume(&mut self, _ctx: &mut RuntimeCtx) {}

    /// Simulation for the top state only; returns a transition applied after the tick (§3.3).
    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition;

    /// When `true`, [`StateStack`] calls [`update_background`](Self::update_background) each tick
    /// while this state is covered by another.
    fn wants_background_update(&self) -> bool {
        false
    }

    /// Optional tick work while not on top (e.g. overworld animation under dialogue).
    fn update_background(&mut self, _ctx: &mut RuntimeCtx) {}

    /// Draws this state; called for a contiguous slice of the stack (see [`StateStack::render`]).
    fn render(&self, renderer: &mut Renderer, alpha: f32);

    /// When `false`, rendering includes opaque states below this one (§4.1).
    fn is_opaque(&self) -> bool {
        true
    }
}

/// Deferred stack mutation requested by the active state's [`update`](GameState::update).
pub enum Transition {
    /// No stack change.
    None,
    /// Push a new state on top after the tick completes.
    Push(Box<dyn GameState>),
    /// Pop the top state.
    Pop,
    /// Replace the top state without calling [`GameState::on_resume`] on the state below.
    Replace(Box<dyn GameState>),
    /// Pop until the top state's [`GameState::id`] matches.
    PopTo(String),
    /// Exit the main loop after the tick completes.
    Quit,
}
