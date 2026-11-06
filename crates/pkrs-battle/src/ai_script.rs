//! Scripted trainer AI port (§7.5 Lua AI, Plugin AI).
//!
//! The battle core never runs scripts. It defines the [`AiScript`] port, which `pkrs-script`
//! implements by calling a Lua `choose_action(view)`, and [`ScriptedAi`], which wraps any
//! script in the ordinary [`BattleAi`] interface:
//!
//! - the script sees an [`AiScriptView`]: an owned, read-only snapshot filtered by the
//!   profile's [`AiVisibility`] (fair: what a human would know; full: "cheating" bosses);
//! - its [`AiChoice`] is resolved and checked against the real battle state; an error, an
//!   invalid choice or [`AiChoice::Default`] falls back to the profile's built-in tier
//!   ([`TrainerAi`]) and failures are recorded as [`AiFallback`]s;
//! - every choice becomes an ordinary [`BattleAction`], so recorded inputs replay without any
//!   script, exactly like the built-in tiers.

use std::collections::{BTreeMap, BTreeSet};

use pkrs_data::{AiProfile, AiVisibility, MoveDef};

use crate::action::BattleAction;
use crate::ai::{AiView, BattleAi, TrainerAi};
use crate::event::BattleEvent;
use crate::hooks::{CreatureView, SideView, SlotRef};
use crate::model::{CreatureStats, PartyIndex, SideId, SlotIndex};
use crate::rng::RngPolicy;
use crate::stat_stage::StatStages;

/// What the script is asked to decide.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AiRequest {
    /// Choose an action for active `slot` (`SelectActions`).
    Action {
        /// Active slot.
        slot: SlotIndex,
    },
    /// Choose a party member to send into `slot` after a faint (`Replace`).
    Replace {
        /// Active slot to fill.
        slot: SlotIndex,
    },
}

impl AiRequest {
    /// The active slot the request is about.
    #[must_use]
    pub fn slot(&self) -> SlotIndex {
        match *self {
            Self::Action { slot } | Self::Replace { slot } => slot,
        }
    }
}

/// One move slot as the script sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct AiMoveInfo {
    /// Move slot `0..4` on the creature.
    pub slot: u8,
    /// Move id.
    pub id: String,
    /// Remaining PP (`None` when hidden).
    pub pp: Option<u8>,
    /// Maximum PP (`None` when hidden).
    pub max_pp: Option<u8>,
    /// Move data from the battle pack (`None` for a move the pack does not know).
    pub def: Option<MoveDef>,
}

/// One creature as the script sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct AiCreatureInfo {
    /// Index in its party.
    pub party_index: PartyIndex,
    /// Active slot (`None` on the bench).
    pub slot: Option<SlotIndex>,
    /// Species id.
    pub species: String,
    /// Display name.
    pub name: String,
    /// Types (primary first).
    pub types: Vec<String>,
    /// Level.
    pub level: u8,
    /// HP as a percentage of max HP, rounded up (0 only when fainted). Always visible.
    pub hp_percent: u8,
    /// Exact HP (`None` when hidden).
    pub hp: Option<u32>,
    /// Max HP (`None` when hidden).
    pub max_hp: Option<u32>,
    /// Permanent stats (`None` when hidden).
    pub stats: Option<CreatureStats>,
    /// Stat stages (visible: the game announces every change).
    pub stages: StatStages,
    /// `true` at 0 HP.
    pub fainted: bool,
    /// Known moves: all of them for the AI's own creatures and under full visibility, only
    /// the ones used so far for the foe under fair visibility.
    pub moves: Vec<AiMoveInfo>,
}

/// One side as the script sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct AiSideInfo {
    /// Which side.
    pub side: SideId,
    /// Trainer / player label.
    pub name: String,
    /// Party index in each active slot (`None` = empty).
    pub active: Vec<Option<PartyIndex>>,
    /// Visible party members, in party order (the foe's unrevealed bench is left out under
    /// fair visibility).
    pub party: Vec<AiCreatureInfo>,
    /// Size of the whole party (shown by the party indicator).
    pub party_size: usize,
}

impl AiSideInfo {
    /// Visible member with `party_index`.
    #[must_use]
    pub fn member(&self, party_index: PartyIndex) -> Option<&AiCreatureInfo> {
        self.party.iter().find(|c| c.party_index == party_index)
    }

    /// Creature in active `slot`.
    #[must_use]
    pub fn active_creature(&self, slot: SlotIndex) -> Option<&AiCreatureInfo> {
        let idx = (*self.active.get(usize::from(slot))?)?;
        self.member(idx)
    }
}

/// What foe information the AI has picked up so far (sent-out creatures, used moves).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiMemory {
    seen: BTreeSet<PartyIndex>,
    revealed: BTreeMap<PartyIndex, BTreeSet<String>>,
    foe_slots: BTreeMap<SlotIndex, PartyIndex>,
}

impl AiMemory {
    /// Records what `side`'s AI learns about the foe from `events` (switches and used moves).
    pub fn observe(&mut self, side: SideId, events: &[BattleEvent]) {
        let foe = side.other();
        for event in events {
            match event {
                BattleEvent::Switched {
                    side: s,
                    slot,
                    to_party,
                } if *s == foe => {
                    self.foe_slots.insert(*slot, *to_party);
                    self.seen.insert(*to_party);
                }
                BattleEvent::MoveUsed {
                    user_side,
                    user_slot,
                    move_id,
                    ..
                } if *user_side == foe => {
                    if let Some(idx) = self.foe_slots.get(user_slot) {
                        self.revealed
                            .entry(*idx)
                            .or_default()
                            .insert(move_id.clone());
                    }
                }
                _ => {}
            }
        }
    }

    /// Marks the foe's current actives as seen.
    pub fn refresh(&mut self, foe: &SideView) {
        for (slot, idx) in foe.active.iter().enumerate() {
            if let Some(idx) = idx {
                self.seen.insert(*idx);
                if let Ok(slot) = SlotIndex::try_from(slot) {
                    self.foe_slots.insert(slot, *idx);
                }
            }
        }
    }

    /// `true` once the foe's `party_index` has been sent out.
    #[must_use]
    pub fn has_seen(&self, party_index: PartyIndex) -> bool {
        self.seen.contains(&party_index)
    }

    /// `true` once the foe's `party_index` has used `move_id`.
    #[must_use]
    pub fn is_revealed(&self, party_index: PartyIndex, move_id: &str) -> bool {
        self.revealed
            .get(&party_index)
            .is_some_and(|m| m.contains(move_id))
    }
}

/// Read-only snapshot a scripted AI decides from (§7.5).
#[derive(Clone, Debug, PartialEq)]
pub struct AiScriptView {
    /// What to decide.
    pub request: AiRequest,
    /// The side the AI plays.
    pub side: SideId,
    /// 1-based turn counter.
    pub turn: u32,
    /// Active slots per side.
    pub side_size: u8,
    /// The visibility the view was built with.
    pub visibility: AiVisibility,
    /// Items the AI may still use (empty when the profile forbids items).
    pub items: Vec<String>,
    /// The AI's own side (always fully visible).
    pub own: AiSideInfo,
    /// The opposing side, filtered by `visibility`.
    pub foe: AiSideInfo,
}

impl AiScriptView {
    /// Builds the view `view.side`'s script gets for `request`.
    #[must_use]
    pub fn build(
        view: &AiView<'_>,
        request: AiRequest,
        visibility: AiVisibility,
        memory: &AiMemory,
        items: &[String],
    ) -> Self {
        let full = |c: &CreatureView| true_info(view, c, true, |_| true);
        let own = side_info(view.own(), |_| true, full);
        let foe_side = view.foe();
        let foe = match visibility {
            AiVisibility::Full => side_info(foe_side, |_| true, full),
            AiVisibility::Fair => side_info(
                foe_side,
                |c| c.active || memory.has_seen(c.party_index),
                |c| true_info(view, c, false, |id| memory.is_revealed(c.party_index, id)),
            ),
        };
        Self {
            request,
            side: view.side,
            turn: view.battle.turn,
            side_size: view.battle.side_size,
            visibility,
            items: items.to_vec(),
            own,
            foe,
        }
    }

    /// The creature the request is about (`None` in `Replace`, or for an empty slot).
    #[must_use]
    pub fn me(&self) -> Option<&AiCreatureInfo> {
        match self.request {
            AiRequest::Action { slot } => self.own.active_creature(slot),
            AiRequest::Replace { .. } => None,
        }
    }
}

fn side_info(
    side: &SideView,
    visible: impl Fn(&CreatureView) -> bool,
    info: impl Fn(&CreatureView) -> AiCreatureInfo,
) -> AiSideInfo {
    let party = side
        .party
        .iter()
        .filter(|c| visible(c))
        .map(|c| {
            let mut i = info(c);
            i.slot = side
                .active
                .iter()
                .position(|a| *a == Some(c.party_index))
                .and_then(|s| SlotIndex::try_from(s).ok());
            i
        })
        .collect();
    AiSideInfo {
        side: side.side,
        name: side.name.clone(),
        active: side.active.clone(),
        party,
        party_size: side.party.len(),
    }
}

fn hp_percent(hp: u32, max_hp: u32) -> u8 {
    if hp == 0 {
        return 0;
    }
    let max = u64::from(max_hp.max(1));
    let pct = (u64::from(hp) * 100).div_ceil(max);
    pct.clamp(1, 100) as u8
}

/// A creature's info; `exact` exposes HP, stats and PP, `known` filters moves by id.
fn true_info(
    view: &AiView<'_>,
    c: &CreatureView,
    exact: bool,
    known: impl Fn(&str) -> bool,
) -> AiCreatureInfo {
    let moves = c
        .moves
        .iter()
        .enumerate()
        .filter_map(|(i, m)| {
            let m = m.as_ref()?;
            (exact || known(&m.id)).then(|| AiMoveInfo {
                slot: i as u8,
                id: m.id.clone(),
                pp: exact.then_some(m.pp),
                max_pp: exact.then_some(m.max_pp),
                def: view.pack.move_by_id(&m.id).cloned(),
            })
        })
        .collect();
    AiCreatureInfo {
        party_index: c.party_index,
        slot: None,
        species: c.species_id.clone(),
        name: c.nickname.clone(),
        types: c.types.clone(),
        level: c.level,
        hp_percent: hp_percent(c.hp, c.stats.max_hp),
        hp: exact.then_some(c.hp),
        max_hp: exact.then_some(c.stats.max_hp),
        stats: exact.then(|| c.stats.clone()),
        stages: c.stages,
        fainted: c.fainted(),
        moves,
    }
}

/// A move picked by a script: by slot or by id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiMoveRef {
    /// Move slot `0..4`.
    Slot(u8),
    /// Move id (the first slot holding it).
    Id(String),
}

/// What a script decided, before it is checked against the battle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiChoice {
    /// No opinion: let the profile's built-in tier decide.
    Default,
    /// Use a move (`target` defaults to the first living foe).
    Move {
        /// Which move.
        mv: AiMoveRef,
        /// Target slot.
        target: Option<SlotRef>,
    },
    /// Switch to a bench member (the only valid choice for [`AiRequest::Replace`]).
    Switch {
        /// Party index to send out.
        to_party: PartyIndex,
    },
    /// Use an item from the trainer's list (`target_slot` defaults to the acting slot).
    Item {
        /// Item id.
        item_id: String,
        /// Own active slot to use it on.
        target_slot: Option<SlotIndex>,
    },
}

/// The scripted-AI port (§4.5): `pkrs-script` implements it over a Lua `choose_action`.
pub trait AiScript {
    /// Decides `view.request`. `Err` (script error, malformed result, …) falls back to the
    /// profile's tier.
    fn choose(&mut self, view: &AiScriptView) -> Result<AiChoice, String>;
}

impl<S: AiScript + ?Sized> AiScript for Box<S> {
    fn choose(&mut self, view: &AiScriptView) -> Result<AiChoice, String> {
        (**self).choose(view)
    }
}

/// Why a scripted decision was replaced by the built-in tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AiFallbackReason {
    /// The script failed (error, budget, malformed result).
    ScriptError(String),
    /// The script chose something the battle does not allow.
    InvalidAction(String),
}

/// One fallback to the built-in tier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AiFallback {
    /// Turn it happened on.
    pub turn: u32,
    /// The request.
    pub request: AiRequest,
    /// Why.
    pub reason: AiFallbackReason,
}

/// A [`BattleAi`] that asks an [`AiScript`] first and falls back to the profile's tier.
pub struct ScriptedAi<S> {
    script: S,
    fallback: TrainerAi,
    side: SideId,
    visibility: AiVisibility,
    memory: AiMemory,
    fallbacks: Vec<AiFallback>,
}

impl<S: AiScript> ScriptedAi<S> {
    /// Scripted AI for `side` with `profile`'s visibility; the fallback tier rolls on a stream
    /// seeded with `seed` and shares the trainer's `items`.
    #[must_use]
    pub fn new(
        profile: &AiProfile,
        items: Vec<String>,
        seed: u64,
        side: SideId,
        script: S,
    ) -> Self {
        Self {
            script,
            fallback: TrainerAi::new(profile, items, seed),
            side,
            visibility: profile.visibility,
            memory: AiMemory::default(),
            fallbacks: Vec::new(),
        }
    }

    /// Scripted AI for `side` of a battle created with `policy` (fallback stream from
    /// [`RngPolicy::ai_seed`]).
    #[must_use]
    pub fn for_battle(
        profile: &AiProfile,
        items: Vec<String>,
        policy: &RngPolicy,
        side: SideId,
        script: S,
    ) -> Self {
        Self::new(profile, items, policy.ai_seed(side), side, script)
    }

    /// Feeds the events of the last step, so a fair AI learns the foe's switches and moves.
    pub fn observe(&mut self, events: &[BattleEvent]) {
        self.memory.observe(self.side, events);
    }

    /// What the AI knows about the foe.
    #[must_use]
    pub fn memory(&self) -> &AiMemory {
        &self.memory
    }

    /// Every fallback so far, oldest first.
    #[must_use]
    pub fn fallbacks(&self) -> &[AiFallback] {
        &self.fallbacks
    }

    /// The built-in tier used as fallback.
    #[must_use]
    pub fn fallback_ai(&self) -> &TrainerAi {
        &self.fallback
    }

    /// The script.
    pub fn script_mut(&mut self) -> &mut S {
        &mut self.script
    }

    /// The view the script gets for `request`.
    #[must_use]
    pub fn script_view(&mut self, view: &AiView<'_>, request: AiRequest) -> AiScriptView {
        self.memory.refresh(view.foe());
        let items: &[String] = if self.fallback.allows_items() {
            self.fallback.items()
        } else {
            &[]
        };
        AiScriptView::build(view, request, self.visibility, &self.memory, items)
    }

    /// Asks the script; `None` means "use the fallback" (already recorded if it failed).
    fn ask(&mut self, view: &AiView<'_>, request: AiRequest) -> Option<BattleAction> {
        let script_view = self.script_view(view, request);
        let reason = match self.script.choose(&script_view) {
            Ok(AiChoice::Default) => return None,
            Ok(choice) => match resolve_choice(view, request, &choice, &self.fallback) {
                Ok(action) => {
                    if let BattleAction::Item { item_id, .. } = &action {
                        self.fallback.take_item(item_id);
                    }
                    return Some(action);
                }
                Err(e) => AiFallbackReason::InvalidAction(e),
            },
            Err(e) => AiFallbackReason::ScriptError(e),
        };
        self.fallbacks.push(AiFallback {
            turn: view.battle.turn,
            request,
            reason,
        });
        None
    }
}

impl<S: AiScript> BattleAi for ScriptedAi<S> {
    fn choose_action(&mut self, view: &AiView<'_>, slot: SlotIndex) -> BattleAction {
        match self.ask(view, AiRequest::Action { slot }) {
            Some(action) => action,
            None => self.fallback.choose_action(view, slot),
        }
    }

    fn choose_replacement(&mut self, view: &AiView<'_>, slot: SlotIndex) -> PartyIndex {
        match self.ask(view, AiRequest::Replace { slot }) {
            Some(BattleAction::Switch { to_party }) => to_party,
            _ => self.fallback.choose_replacement(view, slot),
        }
    }
}

/// Turns `choice` into a [`BattleAction`] that is legal right now, or says why it is not.
fn resolve_choice(
    view: &AiView<'_>,
    request: AiRequest,
    choice: &AiChoice,
    fallback: &TrainerAi,
) -> Result<BattleAction, String> {
    let slot = request.slot();
    let own = view.own();
    if let AiRequest::Replace { .. } = request {
        return match choice {
            AiChoice::Switch { to_party } => {
                check_bench(own, *to_party).map(|()| BattleAction::Switch {
                    to_party: *to_party,
                })
            }
            other => Err(format!("a replacement must be a switch (got {other:?})")),
        };
    }
    let me = own
        .active_creature(slot)
        .filter(|c| !c.fainted())
        .ok_or_else(|| format!("no living creature in own slot {slot}"))?;
    match choice {
        AiChoice::Default => Err("no choice".into()),
        AiChoice::Move { mv, target } => {
            let move_slot = match mv {
                AiMoveRef::Slot(s) => *s,
                AiMoveRef::Id(id) => me
                    .moves
                    .iter()
                    .position(|m| m.as_ref().is_some_and(|m| m.id == *id))
                    .map(|i| i as u8)
                    .ok_or_else(|| format!("{} does not know `{id}`", me.nickname))?,
            };
            let known = me
                .moves
                .get(usize::from(move_slot))
                .and_then(Option::as_ref)
                .ok_or_else(|| format!("move slot {move_slot} is empty"))?;
            if known.pp == 0 {
                return Err(format!("`{}` has no PP left", known.id));
            }
            let target = match target {
                Some(t) => {
                    if t.slot >= view.battle.side_size {
                        return Err(format!("target slot {} out of range", t.slot));
                    }
                    view.battle
                        .creature(*t)
                        .filter(|c| !c.fainted())
                        .ok_or_else(|| format!("no living target at {t:?}"))?;
                    *t
                }
                None => SlotRef::new(
                    view.side.other(),
                    view.foes().first().map_or(0, |(s, _)| *s),
                ),
            };
            Ok(BattleAction::Fight {
                move_slot,
                target_side: target.side,
                target_slot: target.slot,
            })
        }
        AiChoice::Switch { to_party } => {
            check_bench(own, *to_party).map(|()| BattleAction::Switch {
                to_party: *to_party,
            })
        }
        AiChoice::Item {
            item_id,
            target_slot,
        } => {
            if !fallback.allows_items() {
                return Err("this profile may not use items".into());
            }
            if !fallback.items().iter().any(|i| i == item_id) {
                return Err(format!("no `{item_id}` left"));
            }
            let target_slot = target_slot.unwrap_or(slot);
            own.active_creature(target_slot)
                .filter(|c| !c.fainted())
                .ok_or_else(|| format!("no living creature in own slot {target_slot}"))?;
            Ok(BattleAction::Item {
                item_id: item_id.clone(),
                target_side: view.side,
                target_slot,
            })
        }
    }
}

fn check_bench(own: &SideView, to_party: PartyIndex) -> Result<(), String> {
    let c = own
        .party
        .get(usize::from(to_party))
        .ok_or_else(|| format!("no party member {to_party}"))?;
    if c.fainted() {
        return Err(format!("{} has fainted", c.nickname));
    }
    if c.active {
        return Err(format!("{} is already active", c.nickname));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hp_percent_rounds_up_and_only_hits_zero_when_fainted() {
        assert_eq!(hp_percent(0, 50), 0);
        assert_eq!(hp_percent(1, 500), 1);
        assert_eq!(hp_percent(49, 50), 98);
        assert_eq!(hp_percent(50, 50), 100);
        assert_eq!(hp_percent(5, 0), 100);
    }

    #[test]
    fn memory_tracks_switches_and_moves_of_the_foe_only() {
        let mut m = AiMemory::default();
        m.observe(
            SideId::Opponent,
            &[
                BattleEvent::Switched {
                    side: SideId::Player,
                    slot: 0,
                    to_party: 2,
                },
                BattleEvent::MoveUsed {
                    user_side: SideId::Player,
                    user_slot: 0,
                    move_id: "ember".into(),
                    target_side: SideId::Opponent,
                    target_slot: 0,
                },
                BattleEvent::Switched {
                    side: SideId::Opponent,
                    slot: 0,
                    to_party: 1,
                },
            ],
        );
        assert!(m.has_seen(2));
        assert!(!m.has_seen(1), "own switches are not foe info");
        assert!(m.is_revealed(2, "ember"));
        assert!(!m.is_revealed(2, "tackle"));
    }
}
