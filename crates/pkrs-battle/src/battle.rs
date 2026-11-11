//! Battle state machine (§7.1–§7.2) with the §7.4 hook points.

use std::collections::{BTreeMap, BTreeSet};

use crate::accuracy::roll_accuracy;
use crate::action::{BattleAction, BattleInput, SideInput};
use crate::damage::{DamageCalc, DamageInputs, effective_defense, effective_offense};
use crate::effects::apply_effects;
use crate::error::{BattleError, Result};
use crate::event::BattleEvent;
use crate::exp::BattleRewards;
use crate::hooks::{
    ActionMenu, ActionOrder, BattleHookCtx, BattleHooks, BattleValue, BattleView, CreatureView,
    CurrentMove, CustomAction, DamageTaken, FaintEvent, HookCommand, HookResult, MoveRequest,
    MoveResult, MoveView, SideView, SlotRef, StatusApply, SwitchIn, SwitchOut,
};
use crate::model::{
    ActiveCreature, BattleConfig, BattleOutcome, BattleResult, BattleSetup, BattleSideSetup,
    PartyIndex, SideId, SlotIndex,
};
use crate::order::{QueuedAction, resolve_order};
use crate::phase::BattlePhase;
use crate::resources::BattleResources;
use crate::rng::{BattleRng, RngPolicy};
use crate::stat_stage::StatStages;
use pkrs_data::{MoveCategory, MoveEffect};

type ValueMap = BTreeMap<String, BattleValue>;

struct SideState {
    name: String,
    party: Vec<ActiveCreature>,
    active: Vec<Option<PartyIndex>>,
    slot_stages: Vec<StatStages>,
    slot_volatiles: Vec<ValueMap>,
    state: ValueMap,
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
            if let Some(i) = idx
                && usize::from(*i) >= party.len()
            {
                return Err(BattleError::InvalidAction(format!(
                    "{} slot {slot} references missing party index {i}",
                    setup.name
                )));
            }
        }
        Ok(Self {
            name: setup.name,
            party,
            active,
            slot_stages: vec![StatStages::zero(); side_size as usize],
            slot_volatiles: vec![ValueMap::new(); side_size as usize],
            state: ValueMap::new(),
        })
    }

    fn party_creature_and_stages(
        &mut self,
        slot: SlotIndex,
    ) -> Result<(&mut ActiveCreature, &mut StatStages)> {
        let party_idx = self
            .active
            .get(slot as usize)
            .copied()
            .flatten()
            .ok_or_else(|| BattleError::InvalidAction("empty slot".into()))?;
        let creature = self
            .party
            .get_mut(party_idx as usize)
            .ok_or_else(|| BattleError::InvalidAction("party index out of range".into()))?;
        let stages = self
            .slot_stages
            .get_mut(slot as usize)
            .ok_or_else(|| BattleError::InvalidAction("slot stages missing".into()))?;
        Ok((creature, stages))
    }

    fn stages_at(&self, slot: SlotIndex) -> StatStages {
        self.slot_stages
            .get(slot as usize)
            .copied()
            .unwrap_or_default()
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
        let active_set: BTreeSet<_> = self.active.iter().filter_map(|i| *i).collect();
        self.party
            .iter()
            .enumerate()
            .any(|(i, c)| !c.fainted() && !active_set.contains(&(i as u8)))
    }

    fn side_has_any_usable(&self) -> bool {
        self.party.iter().any(|c| !c.fainted())
    }

    fn view(&self, side: SideId) -> SideView {
        let party = self
            .party
            .iter()
            .enumerate()
            .map(|(i, c)| {
                let idx = i as PartyIndex;
                let slot = self.active.iter().position(|a| *a == Some(idx));
                let t = &c.template;
                CreatureView {
                    party_index: idx,
                    species_id: t.species_id.clone(),
                    nickname: t.nickname.clone(),
                    types: t.types.clone(),
                    level: t.level,
                    hp: c.current_hp,
                    stats: t.stats.clone(),
                    stages: slot.map_or_else(StatStages::zero, |s| self.slot_stages[s]),
                    moves: t.moves.clone().map(|m| {
                        m.map(|m| MoveView {
                            id: m.id,
                            pp: m.pp,
                            max_pp: m.max_pp,
                            priority: m.priority,
                            power: m.power,
                        })
                    }),
                    volatiles: slot
                        .and_then(|s| self.slot_volatiles.get(s).cloned())
                        .unwrap_or_default(),
                    active: slot.is_some(),
                }
            })
            .collect();
        SideView {
            side,
            name: self.name.clone(),
            active: self.active.clone(),
            party,
            state: self.state.clone(),
        }
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
    /// Party index acting from each queued slot when the order was locked.
    queue_actors: Vec<Option<PartyIndex>>,
    queue_index: usize,
    result: Option<BattleResult>,
    rng: BattleRng,
    resources: BattleResources,
    player_actions: Vec<(SlotIndex, BattleAction)>,
    opponent_actions: Vec<(SlotIndex, BattleAction)>,
    pending_outcome: Option<BattleOutcome>,
    /// Active weather id and turns remaining.
    weather: Option<(String, u8)>,
    /// Events produced outside `step` (e.g. by `on_action_menu`), emitted by the next `step`.
    deferred: Vec<BattleEvent>,
    /// Move being executed (exposed to hooks through [`BattleView::current_move`]).
    current_move: Option<CurrentMove>,
    /// Default rewards of a player win, before `on_battle_end`.
    rewards: BattleRewards,
}

impl Battle {
    /// Creates a battle in the [`BattlePhase::Start`] phase.
    pub fn new(
        setup: BattleSetup,
        config: BattleConfig,
        resources: BattleResources,
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
            queue_actors: Vec::new(),
            queue_index: 0,
            result: None,
            rng: BattleRng::new(rng_policy),
            resources,
            player_actions: Vec::new(),
            opponent_actions: Vec::new(),
            pending_outcome: None,
            weather: None,
            deferred: Vec::new(),
            current_move: None,
            rewards: BattleRewards::default(),
        })
    }

    /// Sets the default rewards of a player win (none by default). [`BattleResult`] starts
    /// with them when the player wins, before `on_battle_end` hooks run (§7.4); other
    /// outcomes start with no rewards.
    pub fn set_rewards(&mut self, rewards: BattleRewards) {
        self.rewards = rewards;
    }

    /// The default rewards of a player win.
    pub fn rewards(&self) -> BattleRewards {
        self.rewards
    }

    /// Current phase.
    pub fn phase(&self) -> BattlePhase {
        self.phase
    }

    /// Current turn (0 before the first [`BattleEvent::TurnStarted`]).
    pub fn turn(&self) -> u32 {
        self.turn
    }

    /// Battle configuration.
    pub fn config(&self) -> BattleConfig {
        self.config
    }

    /// Final result after [`BattlePhase::End`].
    pub fn result(&self) -> Option<&BattleResult> {
        self.result.as_ref()
    }

    /// Battle RNG.
    pub fn rng(&mut self) -> &mut BattleRng {
        &mut self.rng
    }

    /// Content the battle resolves moves against.
    pub fn resources(&self) -> &BattleResources {
        &self.resources
    }

    /// Read-only snapshot of the field (hooks, AI, UI).
    pub fn view(&self) -> BattleView {
        BattleView {
            phase: self.phase,
            turn: self.turn,
            side_size: self.config.side_size,
            sides: [
                self.player.view(SideId::Player),
                self.opponent.view(SideId::Opponent),
            ],
            current_move: self.current_move.clone(),
        }
    }

    /// The action menu for `slot` (`fight`, `bag`, `party`, `run` plus hook entries, §7.4
    /// `on_action_menu`). Events produced by the hook are emitted by the next [`step`](Self::step).
    pub fn action_menu(
        &mut self,
        slot: SlotRef,
        hooks: &mut dyn BattleHooks,
    ) -> Result<ActionMenu> {
        if self.side(slot.side).creature(slot.slot).is_none() {
            return Err(BattleError::InvalidAction(format!(
                "no active creature on {:?} slot {}",
                slot.side, slot.slot
            )));
        }
        let mut menu = ActionMenu::builtin(slot);
        let mut out = std::mem::take(&mut self.deferred);
        let result = self.run_hook(&mut out, "on_action_menu", |ctx| {
            hooks.on_action_menu(ctx, &mut menu)
        });
        self.deferred = out;
        result?;
        Ok(menu)
    }

    /// Advances the state machine, returning new log entries.
    pub fn step(
        &mut self,
        input: BattleInput,
        hooks: &mut dyn BattleHooks,
    ) -> Result<Vec<BattleEvent>> {
        let mut out = std::mem::take(&mut self.deferred);
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
                    self.perform_switch(side, slot, *to_party, true, hooks, &mut out)?;
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

    /// Runs one hook: optional trace event, then the hook with a fresh context, then the
    /// state changes it requested (§7.4).
    fn run_hook<R>(
        &mut self,
        out: &mut Vec<BattleEvent>,
        name: &'static str,
        f: impl FnOnce(&mut BattleHookCtx<'_>) -> Result<R>,
    ) -> Result<R> {
        if self.config.trace_hooks {
            out.push(BattleEvent::HookTrace { hook: name.into() });
        }
        let view = self.view();
        let (result, commands) = {
            let mut ctx = BattleHookCtx::new(&view, out, self.rng.as_game_rng());
            let result = f(&mut ctx);
            (result, ctx.into_commands())
        };
        let value = result?;
        self.apply_commands(commands, out);
        Ok(value)
    }

    fn apply_commands(&mut self, commands: Vec<HookCommand>, out: &mut Vec<BattleEvent>) {
        for cmd in commands {
            match cmd {
                HookCommand::SetVolatile(slot, key, value) => {
                    let side = self.side_mut(slot.side);
                    if side.creature(slot.slot).is_none() {
                        continue;
                    }
                    let Some(map) = side.slot_volatiles.get_mut(usize::from(slot.slot)) else {
                        continue;
                    };
                    match &value {
                        Some(v) => {
                            map.insert(key.clone(), v.clone());
                        }
                        None => {
                            if map.remove(&key).is_none() {
                                continue;
                            }
                        }
                    }
                    out.push(BattleEvent::VolatileChanged {
                        side: slot.side,
                        slot: slot.slot,
                        key,
                        value,
                    });
                }
                HookCommand::SetSideState(side_id, key, value) => {
                    let side = self.side_mut(side_id);
                    match &value {
                        Some(v) => {
                            side.state.insert(key.clone(), v.clone());
                        }
                        None => {
                            if side.state.remove(&key).is_none() {
                                continue;
                            }
                        }
                    }
                    out.push(BattleEvent::SideStateChanged {
                        side: side_id,
                        key,
                        value,
                    });
                }
                HookCommand::Message(key, args) => out.push(BattleEvent::Message { key, args }),
                HookCommand::Fx(slot, id) => {
                    if self.side(slot.side).creature(slot.slot).is_none() {
                        continue;
                    }
                    out.push(BattleEvent::Fx {
                        side: slot.side,
                        slot: slot.slot,
                        id,
                    });
                }
            }
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
        let mut switch_targets = BTreeSet::new();
        for (slot, action) in input.slots.iter().enumerate() {
            let slot = u8::try_from(slot)
                .map_err(|_| BattleError::InvalidAction("slot index overflow".into()))?;
            if side_state.creature(slot).is_none() {
                return Err(BattleError::InvalidAction(format!(
                    "no active creature on {side:?} slot {slot}"
                )));
            }
            if let BattleAction::Switch { to_party } = action {
                let target = side_state
                    .party
                    .get(usize::from(*to_party))
                    .ok_or_else(|| BattleError::InvalidAction("party index out of range".into()))?;
                if target.fainted() {
                    return Err(BattleError::InvalidAction(
                        "cannot switch to fainted creature".into(),
                    ));
                }
                if side_state.active.contains(&Some(*to_party)) || !switch_targets.insert(*to_party)
                {
                    return Err(BattleError::InvalidAction(
                        "creature already active or chosen twice".into(),
                    ));
                }
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
                self.run_hook(out, "on_battle_start", |ctx| hooks.on_battle_start(ctx))?;
                out.push(BattleEvent::BattleStarted);
                self.turn = 1;
                out.push(BattleEvent::TurnStarted { turn: self.turn });
                self.run_hook(out, "on_turn_start", |ctx| hooks.on_turn_start(ctx))?;
                self.phase = BattlePhase::SelectActions;
            }
            BattlePhase::PreTurn => {
                self.run_hook(out, "on_pre_turn", |ctx| hooks.on_pre_turn(ctx))?;
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
                self.run_hook(out, "on_action_order", |ctx| {
                    hooks.on_action_order(ctx, &mut order)
                })?;
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
                self.queue_actors = self
                    .queue
                    .iter()
                    .map(|q| self.side(q.side).party_index(q.slot))
                    .collect();
                self.queue_index = 0;
                self.phase = BattlePhase::ExecuteAction;
                self.run_current_action(hooks, out)?;
            }
            BattlePhase::ExecuteAction => {
                self.run_current_action(hooks, out)?;
            }
            BattlePhase::FaintCheck => {
                if let Some((side, slot)) = self.next_replace_needed() {
                    self.phase = BattlePhase::Replace { side, slot };
                } else if self.queue_index < self.queue.len() {
                    self.phase = BattlePhase::ExecuteAction;
                    self.run_current_action(hooks, out)?;
                } else if let Some(outcome) = self.check_battle_end() {
                    self.finish(outcome, hooks, out)?;
                } else {
                    self.phase = BattlePhase::EndOfTurn;
                }
            }
            BattlePhase::EndOfTurn => {
                self.run_hook(out, "on_end_of_turn", |ctx| hooks.on_end_of_turn(ctx))?;
                crate::status::process_end_of_turn(self, out)?;
                out.push(BattleEvent::TurnEnded { turn: self.turn });
                if let Some(outcome) = self.check_battle_end() {
                    self.finish(outcome, hooks, out)?;
                } else if let Some((side, slot)) = self.next_replace_needed() {
                    self.phase = BattlePhase::Replace { side, slot };
                } else if self.side(SideId::Player).side_has_any_usable()
                    && self.side(SideId::Opponent).side_has_any_usable()
                {
                    self.turn = self.turn.saturating_add(1);
                    out.push(BattleEvent::TurnStarted { turn: self.turn });
                    self.run_hook(out, "on_turn_start", |ctx| hooks.on_turn_start(ctx))?;
                    self.player_actions.clear();
                    self.opponent_actions.clear();
                    self.queue.clear();
                    self.queue_actors.clear();
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

    /// `true` if the creature that chose the queued action is still in its slot and able to act.
    fn actor_can_act(&self, index: usize, action: &QueuedAction) -> bool {
        let locked = self.queue_actors.get(index).copied().flatten();
        let side = self.side(action.side);
        locked.is_some()
            && side.party_index(action.slot) == locked
            && side.creature(action.slot).is_some_and(|c| !c.fainted())
    }

    fn run_current_action(
        &mut self,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        if self.queue_index >= self.queue.len() {
            self.phase = BattlePhase::FaintCheck;
            return Ok(());
        }
        let action = self.queue[self.queue_index].clone();
        if !self.actor_can_act(self.queue_index, &action) {
            // Fainted (or replaced) before its turn came: the action is dropped.
            self.queue_index = self.queue_index.saturating_add(1);
            self.phase = BattlePhase::FaintCheck;
            return Ok(());
        }
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
            } => {
                let user = SlotRef::new(action.side, action.slot);
                let chosen = SlotRef::new(target_side, target_slot);
                self.execute_fight(user, move_slot, chosen, hooks, out)?;
            }
            BattleAction::Switch { to_party } => {
                self.perform_switch(action.side, action.slot, to_party, false, hooks, out)?;
            }
            BattleAction::Item { item_id, .. } => {
                out.push(BattleEvent::message(format!("item_used:{item_id}")));
            }
            BattleAction::Custom { id, payload } => {
                let mut custom = CustomAction {
                    actor: SlotRef::new(action.side, action.slot),
                    id,
                    payload,
                    handled: false,
                };
                self.run_hook(out, "on_custom_action", |ctx| {
                    hooks.on_custom_action(ctx, &mut custom)
                })?;
                out.push(BattleEvent::CustomActionUsed {
                    side: action.side,
                    slot: action.slot,
                    id: custom.id,
                    handled: custom.handled,
                });
            }
        }
        self.queue_index = self.queue_index.saturating_add(1);
        self.phase = BattlePhase::FaintCheck;
        Ok(())
    }

    fn move_id_at(&self, user: SlotRef, move_slot: u8) -> Option<String> {
        self.side(user.side)
            .creature(user.slot)?
            .template
            .moves
            .get(usize::from(move_slot))?
            .as_ref()
            .map(|m| m.id.clone())
    }

    fn execute_fight(
        &mut self,
        user: SlotRef,
        move_slot: u8,
        target: SlotRef,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        let Some(move_id) = self.move_id_at(user, move_slot) else {
            self.current_move = None;
            return Ok(());
        };
        let targets = crate::targeting::fight_targets(self, user, &move_id, target);
        let result = targets.iter().try_for_each(|&t| {
            self.execute_fight_inner(user, move_slot, t, hooks, out)?;
            Ok::<(), BattleError>(())
        });
        self.current_move = None;
        result
    }

    fn execute_fight_inner(
        &mut self,
        user: SlotRef,
        move_slot: u8,
        target: SlotRef,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        let original_id = self
            .move_id_at(user, move_slot)
            .ok_or_else(|| BattleError::InvalidAction("invalid move slot".into()))?;
        let mut req = MoveRequest {
            user,
            move_slot,
            move_id: original_id.clone(),
            target,
        };
        let verdict = self.run_hook(out, "on_before_move", |ctx| {
            hooks.on_before_move(ctx, &mut req)
        })?;
        if let HookResult::Cancel { reason } = verdict {
            out.push(BattleEvent::MoveCancelled {
                user_side: user.side,
                user_slot: user.slot,
                move_id: original_id,
                reason,
            });
            return Ok(());
        }
        // Hooks may retarget or swap the move; invalid changes fall back to the original.
        let (move_slot, target) = match self.move_id_at(user, req.move_slot) {
            Some(_)
                if self
                    .side(req.target.side)
                    .creature(req.target.slot)
                    .is_some() =>
            {
                (req.move_slot, req.target)
            }
            _ => (move_slot, target),
        };
        let move_id = self
            .move_id_at(user, move_slot)
            .ok_or_else(|| BattleError::InvalidAction("invalid move slot".into()))?;

        let (user_level, user_types, user_stats) = {
            let u = self
                .side(user.side)
                .creature(user.slot)
                .ok_or_else(|| BattleError::InvalidAction("user not active".into()))?;
            (
                u.template.level,
                u.template.types.clone(),
                u.template.stats.clone(),
            )
        };

        let move_def = self
            .resources
            .pack
            .move_by_id(&move_id)
            .ok_or_else(|| BattleError::InvalidAction(format!("unknown move `{move_id}`")))?
            .clone();

        let (target_types, target_stats) = {
            let t = self
                .side(target.side)
                .creature(target.slot)
                .ok_or_else(|| BattleError::InvalidAction("invalid target".into()))?;
            (t.template.types.clone(), t.template.stats.clone())
        };

        let rules = self.resources.rules.clone();
        let blocked = Self::status_blocks_move(
            &mut self.player,
            &mut self.opponent,
            &mut self.rng,
            &rules,
            user.side,
            user.slot,
            out,
        )?;
        if blocked {
            return Ok(());
        }

        out.push(BattleEvent::MoveUsed {
            user_side: user.side,
            user_slot: user.slot,
            move_id: move_id.clone(),
            target_side: target.side,
            target_slot: target.slot,
        });
        self.current_move = Some(CurrentMove {
            user,
            target,
            move_slot,
            move_id: move_id.clone(),
        });

        let user_stages = self.side(user.side).stages_at(user.slot);
        let target_stages = self.side(target.side).stages_at(target.slot);

        let mut acc = roll_accuracy(
            move_def.accuracy,
            &user_stages,
            &target_stages,
            self.rng.as_game_rng(),
        );
        self.run_hook(out, "on_accuracy", |ctx| hooks.on_accuracy(ctx, &mut acc))?;
        if !acc.hits {
            out.push(BattleEvent::Missed {
                target_side: target.side,
                target_slot: target.slot,
                move_id: move_id.clone(),
            });
            let result = MoveResult {
                user,
                target,
                move_id,
                hit: false,
                damage: 0,
                target_fainted: false,
            };
            return self.run_hook(out, "on_after_move", |ctx| {
                hooks.on_after_move(ctx, &result)
            });
        }

        let offense = effective_offense(user_stats.attack, &user_stages, move_def.category);
        let defense = effective_defense(target_stats.defense, &target_stages, move_def.category);

        let critical = self.rng.as_game_rng().gen_range_u32(16) == 0;
        let mut amount = 0u32;
        if move_def.category != MoveCategory::Status {
            let inputs = DamageInputs {
                level: user_level,
                power: move_def.power,
                category: move_def.category,
                offense,
                defense,
                move_type: &move_def.type_id,
                user_types: &user_types,
                target_types: &target_types,
                critical,
            };

            let mut calc = DamageCalc::compute(
                inputs,
                &self.resources.pack.type_chart,
                self.rng.as_game_rng(),
            );
            self.run_hook(out, "on_damage_calc", |ctx| {
                hooks.on_damage_calc(ctx, &mut calc)
            })?;
            let calculated = calc.final_damage();
            if calculated > 0 {
                amount = self.deal_damage(
                    DamageTaken {
                        target,
                        source: Some(user),
                        move_id: Some(move_id.clone()),
                        amount: calculated,
                    },
                    hooks,
                    out,
                )?;
            }
        }

        let target_fainted = self
            .side(target.side)
            .creature(target.slot)
            .is_some_and(|c| c.fainted());

        for effect in &move_def.effects {
            if let MoveEffect::ApplyStatus { status, chance } = effect {
                if target_fainted {
                    continue;
                }
                if rules.status(status).is_none() {
                    continue;
                }
                let roll = self.rng.as_game_rng().gen_range_u32(100);
                if roll < u32::from(*chance) {
                    self.apply_status(
                        StatusApply {
                            target,
                            source: Some(user),
                            status: status.clone(),
                        },
                        hooks,
                        out,
                    )?;
                }
                continue;
            }
            let (user_side_state, target_side_state) = match user.side {
                SideId::Player => (&mut self.player, &mut self.opponent),
                SideId::Opponent => (&mut self.opponent, &mut self.player),
            };
            // Self-targeting moves resolve both sides against the user's side.
            if user.side == target.side {
                let (user_creature, user_stages_mut) =
                    user_side_state.party_creature_and_stages(user.slot)?;
                let mut target_copy = user_creature.clone();
                let mut target_stages_copy = *user_stages_mut;
                apply_effects(
                    std::slice::from_ref(effect),
                    user.side,
                    user.slot,
                    target.side,
                    target.slot,
                    amount,
                    target_fainted,
                    user_creature,
                    &mut target_copy,
                    user_stages_mut,
                    &mut target_stages_copy,
                    &rules,
                    &mut self.weather,
                    self.rng.as_game_rng(),
                    out,
                )?;
                continue;
            }
            let (user_creature, user_stages_mut) =
                user_side_state.party_creature_and_stages(user.slot)?;
            let (target_creature, target_stages_mut) =
                target_side_state.party_creature_and_stages(target.slot)?;
            apply_effects(
                std::slice::from_ref(effect),
                user.side,
                user.slot,
                target.side,
                target.slot,
                amount,
                target_fainted,
                user_creature,
                target_creature,
                user_stages_mut,
                target_stages_mut,
                &rules,
                &mut self.weather,
                self.rng.as_game_rng(),
                out,
            )?;
        }

        let result = MoveResult {
            user,
            target,
            move_id,
            hit: true,
            damage: amount,
            target_fainted,
        };
        self.run_hook(out, "on_after_move", |ctx| {
            hooks.on_after_move(ctx, &result)
        })
    }

    /// `on_damage_taken`, then HP loss, `Damage`, and `Fainted` + `on_faint` if it drops to 0.
    /// Returns the damage actually dealt.
    fn deal_damage(
        &mut self,
        mut hit: DamageTaken,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<u32> {
        let verdict = self.run_hook(out, "on_damage_taken", |ctx| {
            hooks.on_damage_taken(ctx, &mut hit)
        })?;
        let target = hit.target;
        if let HookResult::Cancel { reason } = verdict {
            out.push(BattleEvent::DamagePrevented {
                target_side: target.side,
                target_slot: target.slot,
                reason,
            });
            return Ok(0);
        }
        if hit.amount == 0 {
            return Ok(0);
        }
        let remaining = self.apply_damage(target.side, target.slot, hit.amount)?;
        out.push(BattleEvent::Damage {
            target_side: target.side,
            target_slot: target.slot,
            amount: hit.amount,
            remaining_hp: remaining,
        });
        if remaining == 0
            && let Some(party_index) = self.side(target.side).party_index(target.slot)
        {
            out.push(BattleEvent::Fainted {
                side: target.side,
                slot: target.slot,
                party_index,
            });
            let faint = FaintEvent {
                slot: target,
                party_index,
            };
            self.run_hook(out, "on_faint", |ctx| hooks.on_faint(ctx, &faint))?;
        }
        Ok(hit.amount)
    }

    fn apply_status(
        &mut self,
        mut req: StatusApply,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        let rules = self.resources.rules.clone();
        if rules.status(&req.status).is_none() {
            return Ok(());
        }
        if let Some(creature) = self.side(req.target.side).creature(req.target.slot)
            && let Some(def) = rules.status(&req.status)
            && pkrs_data::status_blocked_by_types(def, &creature.template.types)
        {
            return Ok(());
        }
        let original = req.status.clone();
        let verdict = self.run_hook(out, "on_status_apply", |ctx| {
            hooks.on_status_apply(ctx, &mut req)
        })?;
        match verdict {
            HookResult::Cancel { reason } => out.push(BattleEvent::StatusBlocked {
                target_side: req.target.side,
                target_slot: req.target.slot,
                status: original,
                reason,
            }),
            HookResult::Continue if !req.status.is_empty() => {
                let hook_changed = req.status != original;
                let side = match req.target.side {
                    SideId::Player => &mut self.player,
                    SideId::Opponent => &mut self.opponent,
                };
                let applied =
                    if let Ok((creature, _)) = side.party_creature_and_stages(req.target.slot) {
                        crate::status::apply_status_to_creature(
                            creature,
                            &req.status,
                            &rules,
                            self.rng.as_game_rng(),
                        )
                    } else {
                        false
                    };
                if applied || hook_changed {
                    out.push(BattleEvent::StatusApplied {
                        target_side: req.target.side,
                        target_slot: req.target.slot,
                        status: req.status,
                    });
                }
            }
            HookResult::Continue => {}
        }
        Ok(())
    }

    pub(crate) fn apply_damage(
        &mut self,
        side: SideId,
        slot: SlotIndex,
        amount: u32,
    ) -> Result<u32> {
        let creature = self
            .side_mut(side)
            .creature_mut(slot)
            .ok_or_else(|| BattleError::InvalidAction("invalid target".into()))?;
        creature.current_hp = creature.current_hp.saturating_sub(amount);
        Ok(creature.current_hp)
    }

    /// `on_switch_out` → `on_switch_in` → switch → `Switched`. Hooks can cancel voluntary
    /// switches (trapping); forced replacements after a faint always happen.
    fn perform_switch(
        &mut self,
        side: SideId,
        slot: SlotIndex,
        to_party: PartyIndex,
        forced: bool,
        hooks: &mut dyn BattleHooks,
        out: &mut Vec<BattleEvent>,
    ) -> Result<()> {
        self.validate_switch(side, to_party, forced)?;
        let slot_ref = SlotRef::new(side, slot);
        let mut sw_out = SwitchOut {
            slot: slot_ref,
            party_index: self.side(side).party_index(slot),
            replacement: to_party,
            forced,
        };
        let verdict = self.run_hook(out, "on_switch_out", |ctx| {
            hooks.on_switch_out(ctx, &mut sw_out)
        })?;
        if let (HookResult::Cancel { reason }, false) = (verdict, forced) {
            out.push(BattleEvent::SwitchCancelled { side, slot, reason });
            return Ok(());
        }
        let mut sw_in = SwitchIn {
            slot: slot_ref,
            party_index: to_party,
            forced,
        };
        let verdict = self.run_hook(out, "on_switch_in", |ctx| {
            hooks.on_switch_in(ctx, &mut sw_in)
        })?;
        if let (HookResult::Cancel { reason }, false) = (verdict, forced) {
            out.push(BattleEvent::SwitchCancelled { side, slot, reason });
            return Ok(());
        }
        let side_state = self.side_mut(side);
        side_state.active[slot as usize] = Some(to_party);
        side_state.slot_stages[slot as usize] = StatStages::zero();
        side_state.slot_volatiles[slot as usize].clear();
        out.push(BattleEvent::Switched {
            side,
            slot,
            to_party,
        });
        Ok(())
    }

    fn validate_switch(&self, side: SideId, to_party: PartyIndex, forced: bool) -> Result<()> {
        let side_state = self.side(side);
        let target = side_state
            .party
            .get(usize::from(to_party))
            .ok_or_else(|| BattleError::InvalidAction("party index out of range".into()))?;
        if target.fainted() {
            return Err(BattleError::InvalidAction(
                "cannot switch to fainted creature".into(),
            ));
        }
        if !forced && side_state.active.contains(&Some(to_party)) {
            return Err(BattleError::InvalidAction("creature already active".into()));
        }
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
        let rewards = match outcome {
            BattleOutcome::PlayerWin => self.rewards,
            _ => BattleRewards::default(),
        };
        let mut result = BattleResult {
            outcome,
            turns: self.turn,
            exp: rewards.exp,
            money: rewards.money,
        };
        self.run_hook(out, "on_battle_end", |ctx| {
            hooks.on_battle_end(ctx, &mut result)
        })?;
        out.push(BattleEvent::BattleEnded { outcome });
        self.result = Some(result);
        self.phase = BattlePhase::End;
        Ok(())
    }

    pub(crate) fn rules(&self) -> &pkrs_data::BattleRules {
        &self.resources.rules
    }

    pub(crate) fn weather_snapshot(&self) -> Option<(String, u8)> {
        self.weather.clone()
    }

    pub(crate) fn tick_weather(&mut self) -> bool {
        let Some((_, turns)) = &mut self.weather else {
            return false;
        };
        if *turns > 0 {
            *turns = turns.saturating_sub(1);
        }
        if *turns == 0 {
            self.weather = None;
            return true;
        }
        false
    }

    pub(crate) fn creature(&self, side: SideId, slot: SlotIndex) -> Option<&ActiveCreature> {
        self.side(side).creature(slot)
    }

    pub(crate) fn creature_mut(
        &mut self,
        side: SideId,
        slot: SlotIndex,
    ) -> Option<&mut ActiveCreature> {
        self.side_mut(side).creature_mut(slot)
    }

    pub(crate) fn party_index(&self, side: SideId, slot: SlotIndex) -> Option<PartyIndex> {
        self.side(side).party_index(slot)
    }

    fn status_blocks_move(
        player: &mut SideState,
        opponent: &mut SideState,
        rng: &mut BattleRng,
        rules: &pkrs_data::BattleRules,
        user_side: SideId,
        user_slot: SlotIndex,
        out: &mut Vec<BattleEvent>,
    ) -> Result<bool> {
        let side = match user_side {
            SideId::Player => player,
            SideId::Opponent => opponent,
        };
        let (creature, _) = side.party_creature_and_stages(user_slot)?;
        Ok(crate::status::blocks_move_attempt(
            creature,
            user_side,
            user_slot,
            rules,
            rng.as_game_rng(),
            out,
        ))
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
