//! Minimal snapshot of built-in action edges for Lua `input.pressed("cancel")` (§9.5, §10).

use pkrs_core::{BuiltinAction, InputBuiltinPressed};

/// Per-tick edge flags for built-in actions (script UI).
#[derive(Clone, Debug, Default)]
pub struct ScriptInputSnapshot {
    pressed: [bool; BuiltinAction::ALL.len()],
}

impl ScriptInputSnapshot {
    /// Captures [`InputBuiltinPressed::builtin_pressed`] for every [`BuiltinAction`].
    #[must_use]
    pub fn capture(input: &impl InputBuiltinPressed) -> Self {
        let mut s = Self::default();
        for (i, &builtin) in BuiltinAction::ALL.iter().enumerate() {
            s.pressed[i] = input.builtin_pressed(builtin);
        }
        s
    }

    /// `true` if `name` is a builtin action that fired a press edge this tick.
    #[must_use]
    pub fn pressed_named(&self, name: &str) -> bool {
        BuiltinAction::parse(name).is_some_and(|b| {
            BuiltinAction::ALL
                .iter()
                .position(|&x| x == b)
                .is_some_and(|i| self.pressed[i])
        })
    }
}

/// App data: last snapshot for [`crate::lua::LuaScriptHost::sync_script_input`].
#[derive(Clone, Debug, Default)]
pub(crate) struct LastScriptInput(pub ScriptInputSnapshot);
