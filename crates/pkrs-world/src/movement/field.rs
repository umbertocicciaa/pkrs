//! Field abilities that gate terrain (§6.3).

use std::collections::BTreeSet;

/// Overworld field state consulted by terrain rules: the built-in `surf` flag plus any named
/// states set by scripts or field actions (`player.set_field_state("mud:boots", true)`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FieldState {
    /// Player (or leader) may enter water tiles.
    pub surf: bool,
    /// Other active field states, by name (plugin states are `plugin_id:state`).
    pub states: BTreeSet<String>,
}

impl FieldState {
    /// Name of the built-in water state.
    pub const SURF: &'static str = "surf";

    /// `true` if state `name` is active (`"surf"` reads [`surf`](Self::surf)).
    #[must_use]
    pub fn is_active(&self, name: &str) -> bool {
        if name == Self::SURF {
            self.surf
        } else {
            self.states.contains(name)
        }
    }

    /// Turns state `name` on or off.
    pub fn set(&mut self, name: &str, active: bool) {
        if name == Self::SURF {
            self.surf = active;
        } else if active {
            self.states.insert(name.to_owned());
        } else {
            self.states.remove(name);
        }
    }

    /// Every active state, `surf` included, sorted.
    #[must_use]
    pub fn active(&self) -> Vec<&str> {
        let mut out: Vec<&str> = self.states.iter().map(String::as_str).collect();
        if self.surf {
            out.push(Self::SURF);
            out.sort_unstable();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_states_and_surf() {
        let mut f = FieldState::default();
        f.set("surf", true);
        f.set("mud:boots", true);
        assert!(f.surf && f.is_active("surf") && f.is_active("mud:boots"));
        assert_eq!(f.active(), ["mud:boots", "surf"]);
        f.set("mud:boots", false);
        assert!(!f.is_active("mud:boots"));
    }
}
