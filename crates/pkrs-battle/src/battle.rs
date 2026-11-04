//! Battle state machine (§7.1–§7.2).

use crate::action::{BattleAction, BattleInput, SideInput};
use crate::error::{BattleError, Result};
use crate::event::BattleEvent;
use crate::hooks::{ActionOrder, BattleHookCtx, BattleHooks};
use crate::model::{
    ActiveCreature, BattleConfig, BattleOutcome, BattleResult, BattleSetup, BattleSideSetup,
    PartyIndex, SideId, SlotIndex,
};
use crate::order::{QueuedAction, resolve_order};
use crate::phase::BattlePhase;
use crate::rng::{BattleRng, RngPolicy};

struct SideState {
    party: Vec<ActiveCreature>,
    active: Vec<Option<PartyIndex>>,
}

impl SideState {
    fn from_setup(setup: BattleSideSetup, side_size: u8) -> Result<Self> {
        let party: Vec<_> = setup
            .party
            .into_iter()
            .map(ActiveCreature::from_template)
            .collect();
        if party.is_empty() {
            return Err(BattleError::InvalidAction(format!(
                "{} side has an empty party",
                setup.name
            )));
        }
        let active = setup.leading;
        if active.len() != side_size as usize {
            return Err(BattleError::InvalidAction(format!(
                "{} leading slots must match side_size ({})",
                setup.name, side_size
            )));
        }
        for (slot, idx) in active.iter().enumerate() {
            if let Some(i) = idx {
                if usize::from(*i) >= party.len() {
                    return Err(BattleError::InvalidAction(format!(
                        "{} slot {slot} references missing party index {i}",
                        setup.name
                    )));
                }
            }
        }
        Ok(Self { party, active })
    }

    fn creature(&self, slot: SlotIndex) -> Option<&ActiveCreature> {
        let idx = *self.active.get(slot as usize)?;
        let party_idx = idx?;
        self.party.get(party_idx as usize)
    }

    fn creature_mut(&mut self, slot: SlotIndex) -> Option<&mut ActiveCreature> {
        let idx = *self.active.get(slot as usize)?;
        let party_idx = idx?;
        self.party.get_mut(party_idx as usize)
    }

    fn party_index(&self, slot: SlotIndex) -> Option<PartyIndex> {
        self.active.get(slot as usize).copied().flatten()
    }

    fn has_usable_bench(&self) -> bool {
        let active_set: std::collections::BTreeSet<_> =
            self.active.iter().filter_map(|i| *i).collect();
        self.party
            .iter()
            .enumerate()
            .any(|(i, c)| !c.fainted() && !active_set.contains(&(i as u8)))
    }

    fn side_has_any_usable(&self) -> bool {
        self.party.iter().any(|c| !c.fainted())
    }
}

/// Pure battle simulation.
pub struct Battle {
    config: BattleConfig,
    phase: BattlePhase,
    turn: u32,
    player: SideState,
    opponent: SideState,
    queue: Vec<QueuedAction>,
    queue_index: usize,
    result: Option<BattleResult>,
    rng: BattleRng,
    player_actions: Vec<(SlotIndex, BattleAction)>,
    opponent_actions: Vec<(SlotIndex, BattleAction)>,
    pending_outcome: Option<BattleOutcome>,
}

impl Battle {
    /// Creates a battle in the [`BattlePhase::Start`] phase.
    pub fn new(
        setup: BattleSetup,
        config: BattleConfig,
        rng_policy: RngPolicy,
        _hooks: &mut dyn BattleHooks,
    ) -> Result<Self> {
        let config = config.validate()?;
        let player = SideState::from_setup(setup.player, config.side_size)?;
        let opponent = SideState::from_setup(setup.opponent, config.side_size)?;
        Ok(Self {
            config,
            phase: BattlePhase::Start,
            turn: 0,
            player,
            opponent,
            queue: Vec::new(),
            queue_index: 0,
            result: None,
            rng: BattleRng::new(rng_policy),
            player_actions: Vec::new(),
            opponent_actions: Vec::new(),
            pending_outcome: None,
        })
    }

    /// Current phase.
    pub fn phase(&self) -> BattlePhase {
        self.phase
    }

    /// Current turn (0 before the first [`BattleEvent::TurnStarted`]).
    pub fn turn(&self) -> u32 {
        self.turn
    }

    /// Final result after [`BattlePhase::End`].
    pub fn result(&self) -> Option<&BattleResult> {
        self.result.as_ref()
    }

    /// Battle RNG (damage rolls in later issues).
    pub fn rng(&mut self) -> &mut BattleRng {
        &mut self.rng
    }

    /// Advances the state machine, returning new log entries.
    pub fn step(
        &mut self,
        input: BattleInput,
        hooks: &mut dyn BattleHooks,
    ) -> Result<Vec<BattleEvent>> {
        let mut out = Vec::new();
        loop {
            match self.phase {
                BattlePhase::End => break,
                BattlePhase::SelectActions => {
                    self.take_turn_input(input.clone())?;
                    self.phase = BattlePhase::PreTurn;
                    continue;
                }
                BattlePhase::Replace { side, slot } => {
                    let side_input = match side {
                        SideId::Player => input.player.clone(),
                        SideId::Opponent => input.opponent.clone(),
                    }
                    .ok_or_else(|| BattleError::InvalidAction("missing replace input".into()))?;
                    if side_input.slots.len() != 1 {
                        return Err(BattleError::InvalidAction(
                            "replace requires exactly one switch action".into(),
                        ));
                    }
                    let BattleAction::Switch { to_party } = &side_input.slots[0] else {
                        return Err(BattleError::InvalidAction(
                            "replace requires Switch action".into(),
                        ));
                    };
                    self.apply_switch(side, slot, *to_party, true)?;
                    out.push(BattleEvent::Switched {
                        side,
                        slot,
                        to_party: *to_party,
                    });
                    self.phase = BattlePhase::FaintCheck;
                    continue;
                }
                _ => {
                    self.advance_auto(hooks, &mut out)?;
                    if matches!(
                        self.phase,
                        BattlePhase::SelectActions | BattlePhase::Replace { .. }
                    ) {
                        break;
                    }
                }
            }
        }
        Ok(out)
    }

    fn hook_ctx<'a>(&self, events: &'a [BattleEvent]) -> BattleHookCtx<'a> {
        BattleHookCtx {
            phase: self.phase,
            turn: self.turn,
            side_size: self.config.side_size,
            events,
        }
    }

    fn take_turn_input(&mut self, input: BattleInput) -> Result<()> {
        let player = input.player.ok_or_else(|| {
            BattleError::InvalidAction("missing player actions for SelectActions".into())
        })?;
        let opponent = input.opponent.ok_or_else(|| {
            BattleError::InvalidAction("missing opponent actions for SelectActions".into())
        })?;
        self.store_side_input(SideId::Player, player)?;
        self.store_side_input(SideId::Opponent, opponent)?;
        Ok(())
    }

    fn store_side_input(&mut self, side: SideId, input: SideInput) -> Result<()> {
        if input.slots.len() != self.config.side_size as usize {
            return Err(BattleError::InvalidAction(format!(
                "{side:?} submitted {} actions, expected {}",
                input.slots.len(),
                self.config.side_size
            )));
        }
        let side_state = self.side(side);
        for (slot, action) in input.slots.iter().enumerate() {
            let slot = u8::try_from(slot)
                .map_err(|_| BattleError::InvalidAction("slot index overflow".into()))?;
            if side_state.creature(slot).is_none() {
                return Err(BattleError::InvalidAction(format!(
                    "no active creature on {side:?} slot {slot}"
                )));
            }
            if matches!(action, BattleAction::Switch { .. }) {
                return Err(BattleError::InvalidAction(
                    "switch actions are only valid during Replace".into(),
                ));
            }
        }
        match side {
            SideId::Player => self.player_actions = Self::zip_slots(&input),
            SideId::Opponent => self.opponent_actions = Self::zip_slots(&input),
        }
        Ok(())
    }

    fn zip_slots(input: &SideInput) -> Vec<(SlotIndex, BattleAction)> {
        input
            .slots
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, action)| (i as u8, action))
            .collect()
    }

    fn advance_auto(
        &mut self,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        match self.phase {
            BattlePhase::Start => {
                hooks.on_battle_start(&self.hook_ctx(out))?;
                out.push(BattleEvent::BattleStarted);
                self.turn = 1;
                out.push(BattleEvent::TurnStarted { turn: self.turn });
                hooks.on_turn_start(&self.hook_ctx(out))?;
                self.phase = BattlePhase::SelectActions;
            }
            BattlePhase::PreTurn => {
                hooks.on_pre_turn(&self.hook_ctx(out))?;
                self.phase = BattlePhase::ResolveOrder;
            }
            BattlePhase::ResolveOrder => {
                out.push(BattleEvent::ActionsLocked);
                self.queue = resolve_order(
                    self.config.side_size,
                    &self.player_actions,
                    &self.opponent_actions,
                    |side, slot| self.side(side).creature(slot),
                )?;
                let mut order = ActionOrder {
                    entries: self.queue.iter().map(|q| (q.side, q.slot)).collect(),
                };
                hooks.on_action_order(&self.hook_ctx(out), &mut order)?;
                out.push(BattleEvent::OrderResolved {
                    order: order.entries.clone(),
                });
                if order.entries.len() == self.queue.len() {
                    self.queue.sort_by_key(|q| {
                        order
                            .entries
                            .iter()
                            .position(|&(s, sl)| s == q.side && sl == q.slot)
                            .unwrap_or(usize::MAX)
                    });
                }
                self.queue_index = 0;
                self.phase = BattlePhase::ExecuteAction;
                self.run_current_action(out)?;
            }
            BattlePhase::ExecuteAction => {
                self.run_current_action(out)?;
            }
            BattlePhase::FaintCheck => {
                if let Some((side, slot)) = self.next_replace_needed() {
                    self.phase = BattlePhase::Replace { side, slot };
                } else if self.queue_index < self.queue.len() {
                    self.phase = BattlePhase::ExecuteAction;
                    self.run_current_action(out)?;
                } else if let Some(outcome) = self.check_battle_end() {
                    self.finish(outcome, hooks, out)?;
                } else {
                    self.phase = BattlePhase::EndOfTurn;
                }
            }
            BattlePhase::EndOfTurn => {
                hooks.on_end_of_turn(&self.hook_ctx(out))?;
                out.push(BattleEvent::TurnEnded { turn: self.turn });
                if let Some(outcome) = self.check_battle_end() {
                    self.finish(outcome, hooks, out)?;
                } else if self.side(SideId::Player).side_has_any_usable()
                    && self.side(SideId::Opponent).side_has_any_usable()
                {
                    self.turn = self.turn.saturating_add(1);
                    out.push(BattleEvent::TurnStarted { turn: self.turn });
                    hooks.on_turn_start(&self.hook_ctx(out))?;
                    self.player_actions.clear();
                    self.opponent_actions.clear();
                    self.queue.clear();
                    self.queue_index = 0;
                    self.pending_outcome = None;
                    self.phase = BattlePhase::SelectActions;
                } else {
                    let outcome = if self.side(SideId::Player).side_has_any_usable() {
                        BattleOutcome::PlayerWin
                    } else {
                        BattleOutcome::OpponentWin
                    };
                    self.finish(outcome, hooks, out)?;
                }
            }
            BattlePhase::SelectActions | BattlePhase::Replace { .. } | BattlePhase::End => {}
        }
        Ok(())
    }

    fn run_current_action(&mut self, out: &mut Vec<BattleEvent>) -> Result<()> {
        if self.queue_index >= self.queue.len() {
            self.phase = BattlePhase::FaintCheck;
            return Ok(());
        }
        let action = self.queue[self.queue_index].clone();
        out.push(BattleEvent::ActionCommitted {
            side: action.side,
            slot: action.slot,
            action: action.action.clone(),
        });
        match action.action {
            BattleAction::Run => {
                out.push(BattleEvent::Ran { side: action.side });
                self.pending_outcome = Some(BattleOutcome::Fled { side: action.side });
                self.queue_index = self.queue.len();
            }
            BattleAction::Fight {
                move_slot,
                target_side,
                target_slot,
            } => self.execute_fight(
                action.side,
                action.slot,
                move_slot,
                target_side,
                target_slot,
                out,
            )?,
            BattleAction::Switch { to_party } => {
                self.apply_switch(action.side, action.slot, to_party, false)?;
                out.push(BattleEvent::Switched {
                    side: action.side,
                    slot: action.slot,
                    to_party,
                });
            }
            BattleAction::Item { item_id, .. } => {
                out.push(BattleEvent::Message {
                    key: format!("item_used:{item_id}"),
                });
            }
            BattleAction::Custom { id, .. } => {
                out.push(BattleEvent::Message {
                    key: format!("custom:{id}"),
                });
            }
        }
        self.queue_index = self.queue_index.saturating_add(1);
        self.phase = BattlePhase::FaintCheck;
        Ok(())
    }

    fn execute_fight(
        &mut self,
        user_side: SideId,
        user_slot: SlotIndex,
        move_slot: u8,
        target_side: SideId,
        target_slot: SlotIndex,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        let user = self
            .side(user_side)
            .creature(user_slot)
            .ok_or_else(|| BattleError::InvalidAction("user not active".into()))?;
        let mv = user
            .template
            .moves
            .get(move_slot as usize)
            .and_then(|m| m.as_ref())
            .ok_or_else(|| BattleError::InvalidAction("invalid move slot".into()))?;
        let power = mv.power;
        let atk = user.template.stats.attack;
        out.push(BattleEvent::MoveUsed {
            user_side,
            user_slot,
            move_id: mv.id.clone(),
            target_side,
            target_slot,
        });
        let def = self
            .side(target_side)
            .creature(target_slot)
            .map(|c| c.template.stats.defense)
            .unwrap_or(1);
        let amount = stub_damage(power, atk, def);
        let remaining = self.apply_damage(target_side, target_slot, amount)?;
        out.push(BattleEvent::Damage {
            target_side,
            target_slot,
            amount,
            remaining_hp: remaining,
        });
        if remaining == 0 {
            if let Some(party_index) = self.side(target_side).party_index(target_slot) {
                out.push(BattleEvent::Fainted {
                    side: target_side,
                    slot: target_slot,
                    party_index,
                });
            }
        }
        let _ = self.rng.as_game_rng().next_u32();
        Ok(())
    }

    fn apply_damage(&mut self, side: SideId, slot: SlotIndex, amount: u32) -> Result<u32> {
        let creature = self
            .side_mut(side)
            .creature_mut(slot)
            .ok_or_else(|| BattleError::InvalidAction("invalid target".into()))?;
        creature.current_hp = creature.current_hp.saturating_sub(amount);
        Ok(creature.current_hp)
    }

    fn apply_switch(
        &mut self,
        side: SideId,
        slot: SlotIndex,
        to_party: PartyIndex,
        forced: bool,
    ) -> Result<()> {
        let side_state = self.side_mut(side);
        if usize::from(to_party) >= side_state.party.len() {
            return Err(BattleError::InvalidAction(
                "party index out of range".into(),
            ));
        }
        if side_state.party[to_party as usize].fainted() {
            return Err(BattleError::InvalidAction(
                "cannot switch to fainted creature".into(),
            ));
        }
        if !forced {
            let active: std::collections::BTreeSet<_> =
                side_state.active.iter().filter_map(|i| *i).collect();
            if active.contains(&to_party) {
                return Err(BattleError::InvalidAction("creature already active".into()));
            }
        }
        side_state.active[slot as usize] = Some(to_party);
        Ok(())
    }

    fn next_replace_needed(&self) -> Option<(SideId, SlotIndex)> {
        for side in [SideId::Player, SideId::Opponent] {
            for slot in 0..self.config.side_size {
                if self.side(side).creature(slot)?.fainted() && self.side(side).has_usable_bench() {
                    return Some((side, slot));
                }
            }
        }
        None
    }

    fn check_battle_end(&self) -> Option<BattleOutcome> {
        if let Some(outcome) = self.pending_outcome {
            return Some(outcome);
        }
        if !self.side(SideId::Opponent).side_has_any_usable() {
            return Some(BattleOutcome::PlayerWin);
        }
        if !self.side(SideId::Player).side_has_any_usable() {
            return Some(BattleOutcome::OpponentWin);
        }
        None
    }

    fn finish(
        &mut self,
        outcome: BattleOutcome,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        let mut result = BattleResult {
            outcome,
            turns: self.turn,
        };
        hooks.on_battle_end(&self.hook_ctx(out), &mut result)?;
        out.push(BattleEvent::BattleEnded { outcome });
        self.result = Some(result);
        self.phase = BattlePhase::End;
        Ok(())
    }

    fn side(&self, side: SideId) -> &SideState {
        match side {
            SideId::Player => &self.player,
            SideId::Opponent => &self.opponent,
        }
    }

    fn side_mut(&mut self, side: SideId) -> &mut SideState {
        match side {
            SideId::Player => &mut self.player,
            SideId::Opponent => &mut self.opponent,
        }
    }
}

fn stub_damage(power: u16, attack: u32, defense: u32) -> u32 {
    let defense = defense.max(1);
    ((u32::from(power) * attack / defense) / 2).max(1)
}
