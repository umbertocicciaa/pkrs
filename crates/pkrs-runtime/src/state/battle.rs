//! `Battle` state on the stack (§4.1): wraps a [`BattleScreen`].

use pkrs_battle::NullBattleHooks;
use pkrs_render::Renderer;
use pkrs_script::{WakeSignal, battle_result_value};

use super::{GameState, Transition};
use crate::battle::BattleScreen;
use crate::ctx::RuntimeCtx;

/// Opaque battle state: input goes to the [`BattleScreen`]; when the battle ends, the result
/// is queued for the waiting script ([`WakeSignal::BattleEnded`] with
/// [`battle_result_value`]) and the state pops back to the one below (the overworld).
pub struct BattleState {
    screen: BattleScreen,
}

impl BattleState {
    /// Puts `screen` on the stack.
    #[must_use]
    pub fn new(screen: BattleScreen) -> Self {
        Self { screen }
    }

    /// The screen (tests, debug overlay).
    #[must_use]
    pub fn screen(&self) -> &BattleScreen {
        &self.screen
    }
}

impl GameState for BattleState {
    fn id(&self) -> &str {
        "battle"
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        let result = match ctx.script_host.as_ref() {
            Some(host) => self.screen.update_with_scripts(&mut ctx.input, host),
            None => self.screen.update(&mut ctx.input, &mut NullBattleHooks),
        };
        match result {
            Some(result) => {
                ctx.queue_script_wake(WakeSignal::BattleEnded(battle_result_value(&result)));
                Transition::Pop
            }
            None => Transition::None,
        }
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        self.screen.render(renderer);
    }
}
