//! What a scenario sees of a running [`ProjectHost`] (§15.2).
//!
//! The only place the harness reads game state; it goes through the host's public accessors,
//! so changes to how the overworld stores state stay local to this file.

use pkrs_battle::SideId;
use pkrs_script::ScriptValue;
use pkrs_script::scenario::{
    BattleSnapshot, BattlerSnapshot, PartyMemberSnapshot, Placement, ScenarioSnapshot,
};

use crate::battle::{BattleScreen, PostBattlePhase};
use crate::overworld::UiRequest;
use crate::project::ProjectHost;

/// State name while the player walks around.
pub const STATE_OVERWORLD: &str = "Overworld";
/// State name while a text box (`ui.say`, a sign) is open.
pub const STATE_DIALOGUE: &str = "Dialogue";
/// State name while a choice list (`ui.choose`, `ui.yes_no`) is open.
pub const STATE_CHOICE: &str = "Choice";
/// State name during a warp transition.
pub const STATE_WARP: &str = "Warp";
/// State name while a battle runs (§7.2), including the exp and move texts after a win.
pub const STATE_BATTLE: &str = "Battle";
/// State name from the first evolution after a battle until play returns to the overworld.
pub const STATE_EVOLUTION: &str = "Evolution";

/// The name `state()` / `expect_state` report for `host`: a running battle (or its
/// evolution scene) wins, then the open dialogue, then a running warp, else the overworld.
#[must_use]
pub fn state_name(host: &ProjectHost) -> &'static str {
    if host
        .post_battle()
        .is_some_and(|flow| flow.phase() == PostBattlePhase::Evolution)
    {
        return STATE_EVOLUTION;
    }
    if host.battle().is_some() {
        return STATE_BATTLE;
    }
    match host.overlay().map(|o| o.request()) {
        Some(UiRequest::Say(_)) => STATE_DIALOGUE,
        Some(UiRequest::Choose { .. }) => STATE_CHOICE,
        None if host.is_warping() => STATE_WARP,
        None => STATE_OVERWORLD,
    }
}

/// A [`ScenarioSnapshot`] of `host` after `host.ticks()` ticks; `dialogues_opened` is tracked
/// by the driver.
#[must_use]
pub fn snapshot(host: &ProjectHost, dialogues_opened: u64) -> ScenarioSnapshot {
    let walker = host.walker();
    let mut snap = ScenarioSnapshot {
        tick: host.ticks(),
        state: state_name(host).to_owned(),
        map: Some(host.scene().map().id.clone()),
        language: Some(host.language()),
        player: Some(Placement {
            pos: walker.pos(),
            facing: walker.facing(),
        }),
        player_idle: walker.is_idle(),
        dialogues_opened,
        actions: host.input().action_names().into_iter().collect(),
        ..ScenarioSnapshot::default()
    };
    snap.party = party_snapshot(host);
    if let Some(flow) = host.post_battle() {
        snap.dialogue = flow.text().map(str::to_owned);
        snap.choices = flow.menu_labels();
        snap.cursor = flow.menu_cursor();
        snap.battle = host.battle().map(battle_snapshot);
    } else if let Some(battle) = host.battle() {
        snap.dialogue = battle.text().map(str::to_owned);
        snap.choices = battle.menu_labels();
        snap.cursor = battle.menu_cursor();
        snap.battle = Some(battle_snapshot(battle));
    } else if let Some(overlay) = host.overlay() {
        match overlay.request() {
            UiRequest::Say(text) => snap.dialogue = Some(text.clone()),
            UiRequest::Choose { prompt, options } => {
                snap.dialogue.clone_from(prompt);
                snap.choices.clone_from(options);
                snap.cursor = overlay.cursor();
            }
        }
    }
    if let Some(scripts) = host.scripts() {
        snap.player_locked = scripts.player_locked();
        let world = scripts.world().get();
        snap.flags.clone_from(world.state.flags());
        snap.vars = world
            .state
            .vars()
            .iter()
            .map(|(k, v)| (k.clone(), ScriptValue::from_save_value(Some(v))))
            .collect();
        snap.items.clone_from(world.state.bag());
        snap.npcs = world
            .npcs
            .iter()
            .filter(|n| n.visible)
            .map(|n| {
                (
                    n.id.clone(),
                    Placement {
                        pos: n.walker.pos(),
                        facing: n.walker.facing(),
                    },
                )
            })
            .collect();
    }
    snap
}

/// What `battle()` reports: open menu, turn and both active creatures as displayed.
fn battle_snapshot(battle: &BattleScreen) -> BattleSnapshot {
    let battler = |side: SideId| {
        battle
            .displayed()
            .side(side)
            .active_member(0)
            .map(|m| BattlerSnapshot {
                name: m.name.clone(),
                level: m.level,
                hp: m.hp,
                max_hp: m.max_hp,
            })
    };
    BattleSnapshot {
        menu: battle.menu().map(|m| m.name().to_owned()),
        turn: battle.battle().turn(),
        player: battler(SideId::Player),
        opponent: battler(SideId::Opponent),
    }
}

/// What `party()` reports: every member with its stats at its current level (zero stats
/// when its species is unknown).
fn party_snapshot(host: &ProjectHost) -> Vec<PartyMemberSnapshot> {
    let Some(party) = host.party() else {
        return Vec::new();
    };
    let species = |id: &str| {
        host.battle_content()
            .and_then(|content| content.pack().species.get(id))
    };
    party
        .members()
        .iter()
        .map(|m| {
            let stats = species(&m.species).map(|s| m.stats(s));
            PartyMemberSnapshot {
                species: m.species.clone(),
                name: m.name(),
                level: m.level,
                exp: m.exp,
                moves: m.moves.clone(),
                max_hp: stats.as_ref().map_or(0, |s| s.max_hp),
                attack: stats.as_ref().map_or(0, |s| s.attack),
                defense: stats.as_ref().map_or(0, |s| s.defense),
                speed: stats.as_ref().map_or(0, |s| s.speed),
            }
        })
        .collect()
}
