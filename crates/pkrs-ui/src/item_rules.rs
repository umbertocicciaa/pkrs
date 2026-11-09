//! Field item use rules (built-ins and hook outcomes).

/// Whether an item can be used on a party member in the field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemUseVerdict {
    /// Usable now.
    Allowed,
    /// Wrong pocket or item type.
    WrongContext,
    /// Target already at full HP.
    AlreadyFull,
    /// No registered use handler and no built-in effect.
    NoEffect,
}

/// Built-in field item effects (before plugin hooks).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinItemEffect {
    /// Restores a flat amount of HP.
    Heal(u32),
}

/// Returns a built-in effect for `item_id`, if any.
#[must_use]
pub fn builtin_effect(item_id: &str) -> Option<BuiltinItemEffect> {
    match item_id {
        "potion" => Some(BuiltinItemEffect::Heal(20)),
        "super_potion" => Some(BuiltinItemEffect::Heal(50)),
        "hyper_potion" => Some(BuiltinItemEffect::Heal(200)),
        _ => None,
    }
}

/// Whether `item_id` may target a creature with `current_hp` / `max_hp`.
#[must_use]
pub fn field_use_verdict(
    pocket: &str,
    item_id: &str,
    current_hp: u32,
    max_hp: u32,
    has_hook: bool,
) -> ItemUseVerdict {
    if pocket == "key_items" || pocket == "balls" {
        return ItemUseVerdict::WrongContext;
    }
    if has_hook || builtin_effect(item_id).is_some() {
        if current_hp >= max_hp {
            ItemUseVerdict::AlreadyFull
        } else {
            ItemUseVerdict::Allowed
        }
    } else {
        ItemUseVerdict::NoEffect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn potion_allowed_when_hurt() {
        assert_eq!(
            field_use_verdict("items", "potion", 10, 40, false),
            ItemUseVerdict::Allowed
        );
        assert_eq!(
            field_use_verdict("items", "potion", 40, 40, false),
            ItemUseVerdict::AlreadyFull
        );
        assert_eq!(
            field_use_verdict("key_items", "potion", 10, 40, false),
            ItemUseVerdict::WrongContext
        );
        assert_eq!(
            field_use_verdict("items", "town_map", 10, 40, false),
            ItemUseVerdict::NoEffect
        );
    }
}
