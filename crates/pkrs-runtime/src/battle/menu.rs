//! Battle menu building: action, fight (moves with PP), bag and party menus (§7.4).
//!
//! The action menu comes from [`Battle::action_menu`](pkrs_battle::Battle::action_menu): the
//! built-in `fight` / `bag` / `party` / `run` entries after every `on_action_menu` hook ran,
//! so plugins add, remove or reorder entries there. An entry with any other id is a custom
//! action: picking it submits `BattleAction::Custom { id, payload: [] }`, which the battle
//! hands to `on_custom_action`.
//!
//! Every label is localized through a [`BattleText`] (§5.5): built-in entries use the
//! `battle.menu.*` engine texts, content names their `name` fields.

use std::collections::BTreeMap;

use pkrs_battle::{
    ActionMenu, BattleView, CreatureView, MenuOption, PartyIndex, SideId, SideView, SlotRef,
};
use pkrs_data::TextArgs;

use super::content::display_name;
use crate::locale::BattleText;

/// What an action-menu entry does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// Open the move list.
    Fight,
    /// Open the bag.
    Bag,
    /// Open the party (voluntary switch).
    Party,
    /// Try to run.
    Run,
    /// A hook-added entry: submit `BattleAction::Custom` with this id.
    Custom(String),
}

/// One row of a battle menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuEntry<T> {
    /// Label shown to the player.
    pub label: String,
    /// `false` greys the row out; picking it explains why instead of acting.
    pub enabled: bool,
    /// What picking the row means.
    pub value: T,
}

/// Rows of the action menu, in the order the hooks left them.
#[must_use]
pub fn action_entries(menu: &ActionMenu, text: &BattleText) -> Vec<MenuEntry<ActionKind>> {
    menu.options
        .iter()
        .map(|option| action_entry(option, text))
        .collect()
}

fn action_entry(option: &MenuOption, text: &BattleText) -> MenuEntry<ActionKind> {
    let (label, value) = match option.id.as_str() {
        "fight" => (text.say("battle.menu.fight", &[]), ActionKind::Fight),
        "bag" => (text.say("battle.menu.bag", &[]), ActionKind::Bag),
        "party" => (text.say("battle.menu.party", &[]), ActionKind::Party),
        "run" => (text.say("battle.menu.run", &[]), ActionKind::Run),
        id => (
            custom_label(option, text),
            ActionKind::Custom(id.to_owned()),
        ),
    };
    MenuEntry {
        label,
        enabled: true,
        value,
    }
}

/// A custom entry's label: literal labels as written, `text:` keys localized; an empty label
/// or a missing key falls back to the readable action id.
fn custom_label(option: &MenuOption, text: &BattleText) -> String {
    if option.label.is_empty() {
        return display_name(&option.id).to_uppercase();
    }
    text.localizer()
        .resolve(&option.label, &TextArgs::new())
        .unwrap_or_else(|| display_name(&option.id).to_uppercase())
}

/// `name Lv<level> <hp>/<max_hp>` (`battle.menu.creature`).
fn creature_label(text: &BattleText, name: &str, level: u8, hp: u32, max_hp: u32) -> String {
    text.say(
        "battle.menu.creature",
        &[
            ("name", name),
            ("level", &level.to_string()),
            ("hp", &hp.to_string()),
            ("max_hp", &max_hp.to_string()),
        ],
    )
}

/// Rows of the fight menu: each known move with its PP; moves without PP are disabled.
/// The value is the move slot (`0..4`).
#[must_use]
pub fn fight_entries(creature: &CreatureView, text: &BattleText) -> Vec<MenuEntry<u8>> {
    creature
        .moves
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let m = m.as_ref()?;
            Some(MenuEntry {
                label: text.say(
                    "battle.menu.move",
                    &[
                        ("move", &text.move_name(&m.id)),
                        ("pp", &m.pp.to_string()),
                        ("max_pp", &m.max_pp.to_string()),
                    ],
                ),
                enabled: m.pp > 0,
                value: u8::try_from(i).ok()?,
            })
        })
        .collect()
}

/// Rows of the party menu: every member with level and HP. Fainted and active members are
/// disabled. The value is the party index.
#[must_use]
pub fn party_entries(side: &SideView, text: &BattleText) -> Vec<MenuEntry<PartyIndex>> {
    side.party
        .iter()
        .map(|c| MenuEntry {
            label: creature_label(text, &c.nickname, c.level, c.hp, c.stats.max_hp),
            enabled: !c.active && !c.fainted(),
            value: c.party_index,
        })
        .collect()
}

/// Rows of the bag menu: every item with its count (empty bag: no rows). The value is the
/// item id.
#[must_use]
pub fn bag_entries(bag: &BTreeMap<String, u32>, text: &BattleText) -> Vec<MenuEntry<String>> {
    bag.iter()
        .filter(|(_, n)| **n > 0)
        .map(|(id, n)| MenuEntry {
            label: text.say(
                "battle.menu.item",
                &[("item", &text.item(id)), ("count", &n.to_string())],
            ),
            enabled: true,
            value: id.clone(),
        })
        .collect()
}

/// Foe slots the player can target with a single-target move (doubles).
#[must_use]
pub fn target_entries(view: &BattleView, text: &BattleText) -> Vec<MenuEntry<u8>> {
    (0..view.side_size)
        .filter_map(|slot| {
            let who = view.creature(SlotRef::new(SideId::Opponent, slot))?;
            Some(MenuEntry {
                label: creature_label(text, &who.nickname, who.level, who.hp, who.stats.max_hp),
                enabled: who.hp > 0,
                value: slot,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_battle::{
        Battle, BattleInput, NullBattleHooks, RngPolicy, SideId, SlotRef, battle_testkit,
    };

    fn battle() -> Battle {
        let mut hooks = NullBattleHooks;
        let mut setup = battle_testkit::wild_1v1();
        let mut second = setup.player.party[0].clone();
        second.nickname = "buddy".into();
        setup.player.party.push(second);
        let mut b = Battle::new(
            setup,
            battle_testkit::singles_config(),
            battle_testkit::default_resources(),
            RngPolicy::Fixed { seed: 7 },
            &mut hooks,
        )
        .unwrap();
        b.step(BattleInput::default(), &mut hooks).unwrap();
        b
    }

    #[test]
    fn action_menu_has_builtins_then_hook_entries() {
        let mut menu = ActionMenu::builtin(SlotRef::new(SideId::Player, 0));
        menu.options.insert(
            1,
            MenuOption {
                id: "resonance:resonate".into(),
                label: "text:resonance.menu".into(),
            },
        );
        menu.options.push(MenuOption {
            id: "dance".into(),
            label: "Dance!".into(),
        });
        let entries = action_entries(&menu, &BattleText::default());
        let labels: Vec<&str> = entries.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(
            labels,
            ["FIGHT", "RESONATE", "BAG", "PARTY", "RUN", "Dance!"]
        );
        assert_eq!(
            entries[1].value,
            ActionKind::Custom("resonance:resonate".into())
        );
        assert_eq!(entries[4].value, ActionKind::Run);
        assert!(entries.iter().all(|e| e.enabled));
    }

    #[test]
    fn fight_menu_lists_moves_with_pp() {
        let view = battle().view();
        let mut hero = view
            .creature(SlotRef::new(SideId::Player, 0))
            .unwrap()
            .clone();
        let entries = fight_entries(&hero, &BattleText::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Tackle 35/35");
        assert!(entries[0].enabled);
        hero.moves[0].as_mut().unwrap().pp = 0;
        assert!(!fight_entries(&hero, &BattleText::default())[0].enabled);
    }

    #[test]
    fn party_menu_disables_active_and_fainted_members() {
        let view = battle().view();
        let mut side = view.side(SideId::Player).clone();
        let entries = party_entries(&side, &BattleText::default());
        assert_eq!(entries[0].label, "hero Lv5 40/40");
        assert!(!entries[0].enabled, "active member");
        assert!(entries[1].enabled);
        side.party[1].hp = 0;
        assert!(
            !party_entries(&side, &BattleText::default())[1].enabled,
            "fainted member"
        );
    }

    #[test]
    fn bag_menu_lists_items_with_counts() {
        let bag = BTreeMap::from([("potion".to_owned(), 2), ("empty".to_owned(), 0)]);
        let entries = bag_entries(&bag, &BattleText::default());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].label, "Potion x2");
        assert_eq!(entries[0].value, "potion");
        assert!(bag_entries(&BTreeMap::new(), &BattleText::default()).is_empty());
    }
}
