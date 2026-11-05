//! Game actions mapped from physical keys (§10).

use core::fmt;

use pkrs_core::manifest::is_action_name;

use crate::error::{InputError, InputResult};

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

    pub(crate) fn index(self) -> usize {
        self as u8 as usize
    }
}

/// A built-in or plugin-registered action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Action {
    /// One of the engine built-ins.
    Builtin(BuiltinAction),
    /// A custom action registered through [`Input::register_action`](crate::Input::register_action).
    Custom(CustomActionId),
}

impl Action {
    /// Returns the built-in variant when this action is built-in.
    pub fn as_builtin(self) -> Option<BuiltinAction> {
        match self {
            Self::Builtin(b) => Some(b),
            Self::Custom(_) => None,
        }
    }
}

impl From<BuiltinAction> for Action {
    fn from(value: BuiltinAction) -> Self {
        Self::Builtin(value)
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(b) => f.write_str(b.name()),
            Self::Custom(id) => write!(f, "custom({})", id.get()),
        }
    }
}

/// Dense handle for a plugin-registered action.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CustomActionId(u32);

impl CustomActionId {
    pub(crate) fn new(index: u32) -> Self {
        Self(index)
    }

    /// Returns the registry index.
    pub fn get(self) -> u32 {
        self.0
    }
}

/// Registry of custom action names.
#[derive(Clone, Debug, Default)]
pub(crate) struct ActionRegistry {
    names: Vec<String>,
}

impl ActionRegistry {
    /// Registers `name`, returning its handle.
    ///
    /// Names follow the same rules as `pkrs.toml [input]` (`snake_case` or `plugin:action`).
    pub(crate) fn register(&mut self, name: &str) -> InputResult<CustomActionId> {
        validate_action_name(name)?;
        if self.names.iter().any(|existing| existing == name) {
            return Err(InputError::DuplicateAction(name.to_owned()));
        }
        let index = u32::try_from(self.names.len()).map_err(|_| InputError::RegistryFull)?;
        self.names.push(name.to_owned());
        Ok(CustomActionId::new(index))
    }

    /// Resolves a registered custom action by name.
    pub(crate) fn id_by_name(&self, name: &str) -> Option<CustomActionId> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| CustomActionId::new(i as u32))
    }
}

/// Resolves `name` to a built-in or registered custom [`Action`].
pub(crate) fn resolve_action(name: &str, registry: &ActionRegistry) -> Option<Action> {
    BuiltinAction::parse(name)
        .map(Action::Builtin)
        .or_else(|| registry.id_by_name(name).map(Action::Custom))
}

fn validate_action_name(name: &str) -> InputResult<()> {
    if is_action_name(name) {
        Ok(())
    } else {
        Err(InputError::InvalidActionName(name.to_owned()))
    }
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

    #[test]
    fn register_and_resolve_custom() {
        let mut registry = ActionRegistry::default();
        let id = registry.register("resonance:toggle").expect("valid");
        assert_eq!(
            resolve_action("resonance:toggle", &registry),
            Some(Action::Custom(id))
        );
        assert_eq!(
            resolve_action("confirm", &registry),
            Some(Action::Builtin(BuiltinAction::Confirm))
        );
    }

    #[test]
    fn duplicate_custom_is_rejected() {
        let mut registry = ActionRegistry::default();
        registry.register("foo:bar").expect("first");
        assert!(registry.register("foo:bar").is_err());
    }
}
