//! Built-in input actions shared across crates (§10); the platform `Input` facade maps keys to these.

/// Built-in actions every game reads instead of raw keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[repr(u8)]
#[non_exhaustive]
pub enum BuiltinAction {
    /// Step / menu cursor up.
    Up,
    /// Step / menu cursor down.
    Down,
    /// Step / menu cursor left.
    Left,
    /// Step / menu cursor right.
    Right,
    /// Primary confirm (talk, select menu row, …).
    Confirm,
    /// Back / close.
    Cancel,
    /// Pause / party menu.
    Menu,
    /// Hold while moving to run (overworld).
    Run,
    /// Secondary select / register (when used).
    Select,
}

impl BuiltinAction {
    /// Every built-in action, in declaration order.
    pub const ALL: &'static [BuiltinAction] = &[
        BuiltinAction::Up,
        BuiltinAction::Down,
        BuiltinAction::Left,
        BuiltinAction::Right,
        BuiltinAction::Confirm,
        BuiltinAction::Cancel,
        BuiltinAction::Menu,
        BuiltinAction::Run,
        BuiltinAction::Select,
    ];

    /// Manifest / Lua action name (`up`, `confirm`, …).
    pub fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Confirm => "confirm",
            Self::Cancel => "cancel",
            Self::Menu => "menu",
            Self::Run => "run",
            Self::Select => "select",
        }
    }

    /// Parses a built-in action name.
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "up" => Some(Self::Up),
            "down" => Some(Self::Down),
            "left" => Some(Self::Left),
            "right" => Some(Self::Right),
            "confirm" => Some(Self::Confirm),
            "cancel" => Some(Self::Cancel),
            "menu" => Some(Self::Menu),
            "run" => Some(Self::Run),
            "select" => Some(Self::Select),
            _ => None,
        }
    }

    /// Stable index into [`BuiltinAction::ALL`].
    #[must_use]
    pub fn index(self) -> usize {
        self as u8 as usize
    }
}

/// Engine input that exposes edge-triggered “pressed” for [`BuiltinAction`] (Lua UI, §9.5 §10).
pub trait InputBuiltinPressed {
    /// `true` if `action` had a press edge this simulation tick.
    fn builtin_pressed(&self, action: BuiltinAction) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_names_round_trip() {
        for action in BuiltinAction::ALL {
            assert_eq!(BuiltinAction::parse(action.name()), Some(*action));
        }
    }
}
