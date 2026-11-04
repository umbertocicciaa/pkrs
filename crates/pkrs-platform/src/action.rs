//! Game actions mapped from physical keys (§10).

use core::fmt;

use pkrs_core::validate_id;

use crate::error::{InputError, InputResult};

/// Built-in actions every game reads instead of raw keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
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
        match self {
            Self::Up => 0,
            Self::Down => 1,
            Self::Left => 2,
            Self::Right => 3,
            Self::Confirm => 4,
            Self::Cancel => 5,
            Self::Menu => 6,
            Self::Run => 7,
            Self::Select => 8,
        }
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

/// Registry of custom action names and default keys.
#[derive(Clone, Debug, Default)]
pub struct ActionRegistry {
    names: Vec<String>,
    default_keys: Vec<Vec<crate::Key>>,
}

impl ActionRegistry {
    /// Registers `name` with `default_keys`, returning its handle.
    ///
    /// Names follow the same rules as `pkrs.toml [input]` (`snake_case` or `plugin:action`).
    pub fn register(
        &mut self,
        name: &str,
        default_keys: &[crate::Key],
    ) -> InputResult<CustomActionId> {
        validate_action_name(name)?;
        if self.names.iter().any(|existing| existing == name) {
            return Err(InputError::DuplicateAction(name.to_owned()));
        }
        let index = u32::try_from(self.names.len()).map_err(|_| InputError::RegistryFull)?;
        self.names.push(name.to_owned());
        self.default_keys.push(default_keys.to_vec());
        Ok(CustomActionId::new(index))
    }

    /// Resolves a registered custom action by name.
    pub fn id_by_name(&self, name: &str) -> Option<CustomActionId> {
        self.names
            .iter()
            .position(|n| n == name)
            .map(|i| CustomActionId::new(i as u32))
    }
}

/// Resolves `name` to a built-in or registered custom [`Action`].
pub fn resolve_action(name: &str, registry: &ActionRegistry) -> Option<Action> {
    BuiltinAction::parse(name)
        .map(Action::Builtin)
        .or_else(|| registry.id_by_name(name).map(Action::Custom))
}

fn validate_action_name(name: &str) -> InputResult<()> {
    match name.split_once(':') {
        Some((plugin, action)) => {
            if !is_plugin_id(plugin) {
                return Err(InputError::InvalidActionName(name.to_owned()));
            }
            pkrs_core::validate_id(action)
                .map_err(|_| InputError::InvalidActionName(name.to_owned()))?;
        }
        None => {
            validate_id(name).map_err(|_| InputError::InvalidActionName(name.to_owned()))?;
        }
    }
    Ok(())
}

fn is_plugin_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Key;

    #[test]
    fn builtin_names_round_trip() {
        for action in BuiltinAction::ALL {
            assert_eq!(BuiltinAction::parse(action.name()), Some(*action));
        }
    }

    #[test]
    fn register_and_resolve_custom() {
        let mut registry = ActionRegistry::default();
        let id = registry
            .register("resonance:toggle", &[Key::R])
            .expect("valid");
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
        registry.register("foo:bar", &[]).expect("first");
        assert!(registry.register("foo:bar", &[]).is_err());
    }
}
