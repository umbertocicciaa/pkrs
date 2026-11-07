//! Game actions mapped from physical keys (§10).

use core::fmt;

use pkrs_core::manifest::is_action_name;

use crate::error::{InputError, InputResult};

/// Re-export: built-in action ids (defined in `pkrs-core` for script/runtime sharing).
pub use pkrs_core::BuiltinAction;

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

    /// Registered names, in registration order.
    pub(crate) fn names(&self) -> &[String] {
        &self.names
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
