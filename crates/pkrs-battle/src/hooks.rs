//! Synchronous battle hooks (§7.4); default implementations are no-ops.
//!
//! Every hook receives a [`BattleHookCtx`]: a read-only [`BattleView`] of the field, the event
//! log so far, the battle RNG (policy-aware, §7.2) and a command buffer for the generic state
//! plugins need (volatiles, side state, messages). Mutable payloads ([`MoveRequest`],
//! [`DamageCalc`], [`ActionMenu`], …) are passed alongside. Cancellable hooks return
//! [`HookResult::Cancel`].
//!
//! `pkrs-script` implements this trait by dispatching to Lua (`LuaBattleHooks`); game Lua never
//! implements it directly.

use std::collections::BTreeMap;

use pkrs_core::GameRng;

use crate::accuracy::AccuracyCalc;
use crate::damage::DamageCalc;
use crate::error::Result;
use crate::event::BattleEvent;
use crate::model::{BattleResult, CreatureStats, PartyIndex, SideId, SlotIndex};
use crate::phase::BattlePhase;
use crate::stat_stage::StatStages;

/// An active slot on one side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotRef {
    /// Side.
    pub side: SideId,
    /// Active slot on that side.
    pub slot: SlotIndex,
}

impl SlotRef {
    /// Creates a slot reference.
    #[must_use]
    pub const fn new(side: SideId, slot: SlotIndex) -> Self {
        Self { side, slot }
    }
}

/// Plugin-owned value stored in volatiles and side state (no floats: replays stay exact).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleValue {
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// String.
    Str(String),
    /// List.
    List(Vec<BattleValue>),
    /// String-keyed map (e.g. `{ turns = 3 }`).
    Map(BTreeMap<String, BattleValue>),
}

/// Outcome of a cancellable hook.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum HookResult {
    /// Proceed (with any modifications made to the payload).
    #[default]
    Continue,
    /// Stop the action. `reason` is an optional message key shown by the UI.
    Cancel {
        /// Message key.
        reason: Option<String>,
    },
}

impl HookResult {
    /// Cancel with a message key.
    #[must_use]
    pub fn cancel(reason: impl Into<String>) -> Self {
        Self::Cancel {
            reason: Some(reason.into()),
        }
    }

    /// `true` for [`HookResult::Cancel`].
    #[must_use]
    pub fn is_cancel(&self) -> bool {
        matches!(self, Self::Cancel { .. })
    }
}

/// One known move, as seen by hooks and AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveView {
    /// Move id.
    pub id: String,
    /// Remaining PP.
    pub pp: u8,
    /// Maximum PP.
    pub max_pp: u8,
    /// Priority.
    pub priority: i8,
    /// Base power from the creature's move slot.
    pub power: u16,
}

/// One creature, as seen by hooks and AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreatureView {
    /// Index in its party.
    pub party_index: PartyIndex,
    /// Species id.
    pub species_id: String,
    /// Display name.
    pub nickname: String,
    /// Types (primary first).
    pub types: Vec<String>,
    /// Level.
    pub level: u8,
    /// Current HP.
    pub hp: u32,
    /// Permanent stats.
    pub stats: CreatureStats,
    /// Stat stages (zero unless active).
    pub stages: StatStages,
    /// Move slots.
    pub moves: [Option<MoveView>; 4],
    /// Volatile state (only while active; cleared on switch-out).
    pub volatiles: BTreeMap<String, BattleValue>,
    /// `true` when the creature is in an active slot.
    pub active: bool,
}

impl CreatureView {
    /// `true` at 0 HP.
    #[must_use]
    pub fn fainted(&self) -> bool {
        self.hp == 0
    }
}

/// One side, as seen by hooks and AI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideView {
    /// Which side.
    pub side: SideId,
    /// Trainer / player label.
    pub name: String,
    /// Party index in each active slot (`None` = empty).
    pub active: Vec<Option<PartyIndex>>,
    /// Whole party.
    pub party: Vec<CreatureView>,
    /// Plugin side state (`resonance_used`, …).
    pub state: BTreeMap<String, BattleValue>,
}

impl SideView {
    /// Creature in active `slot`.
    #[must_use]
    pub fn active_creature(&self, slot: SlotIndex) -> Option<&CreatureView> {
        let idx = (*self.active.get(usize::from(slot))?)?;
        self.party.get(usize::from(idx))
    }

    /// Party members that could be switched in (not fainted, not active).
    pub fn bench(&self) -> impl Iterator<Item = &CreatureView> {
        self.party.iter().filter(|c| !c.active && !c.fainted())
    }
}

/// The move being executed while move hooks run (`on_accuracy`, `on_damage_calc`, …).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurrentMove {
    /// User.
    pub user: SlotRef,
    /// Target.
    pub target: SlotRef,
    /// Move slot on the user.
    pub move_slot: u8,
    /// Move id.
    pub move_id: String,
}

/// Read-only snapshot of the battle (§7.4 `BattleHookCtx`, §7.5 AI view).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BattleView {
    /// Current phase.
    pub phase: BattlePhase,
    /// 1-based turn counter (0 before the first turn).
    pub turn: u32,
    /// Active slots per side.
    pub side_size: u8,
    /// `[player, opponent]`.
    pub sides: [SideView; 2],
    /// The move being executed, from `on_accuracy` to `on_after_move` (`None` otherwise).
    pub current_move: Option<CurrentMove>,
}

impl BattleView {
    /// View of `side`.
    #[must_use]
    pub fn side(&self, side: SideId) -> &SideView {
        match side {
            SideId::Player => &self.sides[0],
            SideId::Opponent => &self.sides[1],
        }
    }

    /// Creature in `slot`.
    #[must_use]
    pub fn creature(&self, slot: SlotRef) -> Option<&CreatureView> {
        self.side(slot.side).active_creature(slot.slot)
    }

    /// Every non-fainted active creature, player side first.
    #[must_use]
    pub fn active_creatures(&self) -> Vec<(SlotRef, &CreatureView)> {
        let mut out = Vec::new();
        for side in [SideId::Player, SideId::Opponent] {
            for slot in 0..self.side_size {
                if let Some(c) = self.side(side).active_creature(slot) {
                    if !c.fainted() {
                        out.push((SlotRef::new(side, slot), c));
                    }
                }
            }
        }
        out
    }
}

/// Deferred state change requested by a hook (applied right after it returns).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum HookCommand {
    SetVolatile(SlotRef, String, Option<BattleValue>),
    SetSideState(SideId, String, Option<BattleValue>),
    Message(String),
    Fx(SlotRef, Option<String>),
}

/// What every hook receives (§7.4).
pub struct BattleHookCtx<'a> {
    view: &'a BattleView,
    events: &'a [BattleEvent],
    rng: &'a mut dyn GameRng,
    commands: Vec<HookCommand>,
}

impl<'a> BattleHookCtx<'a> {
    /// Creates a context (the battle core does this; public for hook unit tests).
    pub fn new(view: &'a BattleView, events: &'a [BattleEvent], rng: &'a mut dyn GameRng) -> Self {
        Self {
            view,
            events,
            rng,
            commands: Vec::new(),
        }
    }

    /// Read-only battle snapshot taken when the hook was called.
    #[must_use]
    pub fn view(&self) -> &BattleView {
        self.view
    }

    /// Current phase.
    #[must_use]
    pub fn phase(&self) -> BattlePhase {
        self.view.phase
    }

    /// Current turn.
    #[must_use]
    pub fn turn(&self) -> u32 {
        self.view.turn
    }

    /// Active slots per side.
    #[must_use]
    pub fn side_size(&self) -> u8 {
        self.view.side_size
    }

    /// Events emitted so far in this `step` (append-only).
    #[must_use]
    pub fn events(&self) -> &[BattleEvent] {
        self.events
    }

    /// Uniform roll in `[0, n)` from the battle RNG (`ctx:random(n)` in Lua). Consumes the
    /// battle stream, so it is part of the deterministic replay.
    pub fn random(&mut self, n: u32) -> u32 {
        self.rng.gen_range_u32(n)
    }

    /// Volatile `key` on `slot`, including changes made earlier in this hook.
    #[must_use]
    pub fn volatile(&self, slot: SlotRef, key: &str) -> Option<BattleValue> {
        for cmd in self.commands.iter().rev() {
            if let HookCommand::SetVolatile(s, k, v) = cmd {
                if *s == slot && k == key {
                    return v.clone();
                }
            }
        }
        self.view.creature(slot)?.volatiles.get(key).cloned()
    }

    /// Sets volatile `key` on `slot` (cleared automatically when it switches out).
    pub fn set_volatile(&mut self, slot: SlotRef, key: impl Into<String>, value: BattleValue) {
        self.commands
            .push(HookCommand::SetVolatile(slot, key.into(), Some(value)));
    }

    /// Clears volatile `key` on `slot`.
    pub fn clear_volatile(&mut self, slot: SlotRef, key: impl Into<String>) {
        self.commands
            .push(HookCommand::SetVolatile(slot, key.into(), None));
    }

    /// Side state `key` of `side`, including changes made earlier in this hook.
    #[must_use]
    pub fn side_state(&self, side: SideId, key: &str) -> Option<BattleValue> {
        for cmd in self.commands.iter().rev() {
            if let HookCommand::SetSideState(s, k, v) = cmd {
                if *s == side && k == key {
                    return v.clone();
                }
            }
        }
        self.view.side(side).state.get(key).cloned()
    }

    /// Sets side state `key` (lasts for the whole battle).
    pub fn set_side_state(&mut self, side: SideId, key: impl Into<String>, value: BattleValue) {
        self.commands
            .push(HookCommand::SetSideState(side, key.into(), Some(value)));
    }

    /// Queues a [`BattleEvent::Message`] with `key`.
    pub fn message(&mut self, key: impl Into<String>) {
        self.commands.push(HookCommand::Message(key.into()));
    }

    /// Queues a [`BattleEvent::Fx`] that shows the visual effect `id` on the creature in
    /// `slot` (`ctx.play_fx` in Lua). Ignored if `slot` has no active creature.
    pub fn play_fx(&mut self, slot: SlotRef, id: impl Into<String>) {
        self.commands.push(HookCommand::Fx(slot, Some(id.into())));
    }

    /// Queues a [`BattleEvent::Fx`] that clears the visual effect on the creature in `slot`
    /// (`ctx.clear_fx` in Lua). Ignored if `slot` has no active creature.
    pub fn clear_fx(&mut self, slot: SlotRef) {
        self.commands.push(HookCommand::Fx(slot, None));
    }

    /// Number of state changes requested so far (see [`rollback`](Self::rollback)).
    #[must_use]
    pub fn checkpoint(&self) -> usize {
        self.commands.len()
    }

    /// Drops the state changes requested after `checkpoint`. Script adapters use this so a
    /// handler that fails part-way leaves no trace (§9.6). RNG draws are not undone.
    pub fn rollback(&mut self, checkpoint: usize) {
        self.commands.truncate(checkpoint);
    }

    pub(crate) fn into_commands(self) -> Vec<HookCommand> {
        self.commands
    }
}

/// Mutable ordering list for [`BattleHooks::on_action_order`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActionOrder {
    /// `(side, slot)` entries to execute.
    pub entries: Vec<(SideId, SlotIndex)>,
}

/// A move about to be used (`on_before_move`): hooks may change the move slot or target, or
/// cancel (flinch, sleep, …). Invalid changes are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveRequest {
    /// User.
    pub user: SlotRef,
    /// Move slot on the user (`0..4`).
    pub move_slot: u8,
    /// Move id in that slot (informational; changing `move_slot` updates it).
    pub move_id: String,
    /// Target.
    pub target: SlotRef,
}

/// Damage about to be applied (`on_damage_taken`): hooks may change `amount` or cancel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DamageTaken {
    /// Who is hit.
    pub target: SlotRef,
    /// Who caused it, if a creature did.
    pub source: Option<SlotRef>,
    /// Move that caused it, if any.
    pub move_id: Option<String>,
    /// HP to remove.
    pub amount: u32,
}

/// Result of a move (`on_after_move`, read-only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MoveResult {
    /// User.
    pub user: SlotRef,
    /// Target.
    pub target: SlotRef,
    /// Move id.
    pub move_id: String,
    /// `false` when the move missed.
    pub hit: bool,
    /// Damage dealt by the main hit.
    pub damage: u32,
    /// Target fainted.
    pub target_fainted: bool,
}

/// A status about to be applied (`on_status_apply`): hooks may change it or cancel (immunity).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusApply {
    /// Who receives it.
    pub target: SlotRef,
    /// Who caused it, if a creature did.
    pub source: Option<SlotRef>,
    /// Status id.
    pub status: String,
}

/// A creature entering a slot (`on_switch_in`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchIn {
    /// Slot being filled.
    pub slot: SlotRef,
    /// Party index coming in.
    pub party_index: PartyIndex,
    /// Forced replacement after a faint (cannot be cancelled).
    pub forced: bool,
}

/// A creature leaving a slot (`on_switch_out`): cancel to trap it (voluntary switches only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwitchOut {
    /// Slot being vacated.
    pub slot: SlotRef,
    /// Party index leaving (`None` if the slot was empty).
    pub party_index: Option<PartyIndex>,
    /// Party index replacing it.
    pub replacement: PartyIndex,
    /// Forced replacement after a faint (cannot be cancelled).
    pub forced: bool,
}

/// A creature fainted (`on_faint`, read-only).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FaintEvent {
    /// Slot it fainted in.
    pub slot: SlotRef,
    /// Its party index.
    pub party_index: PartyIndex,
}

/// One entry in the action menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuOption {
    /// Action id: `fight`, `bag`, `party`, `run`, or a plugin id (`resonance:resonate`).
    pub id: String,
    /// Label text key.
    pub label: String,
}

/// The action menu for one slot (`on_action_menu`): hooks may add, remove or reorder entries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionMenu {
    /// Whose menu.
    pub slot: SlotRef,
    /// Entries in display order.
    pub options: Vec<MenuOption>,
}

impl ActionMenu {
    /// The built-in entries.
    #[must_use]
    pub fn builtin(slot: SlotRef) -> Self {
        let opt = |id: &str| MenuOption {
            id: id.into(),
            label: format!("text:battle.menu.{id}"),
        };
        Self {
            slot,
            options: vec![opt("fight"), opt("bag"), opt("party"), opt("run")],
        }
    }

    /// `true` if an entry with `id` exists.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.options.iter().any(|o| o.id == id)
    }
}

/// A [`BattleAction::Custom`](crate::BattleAction::Custom) being executed (`on_custom_action`).
/// The handler that implements `id` sets `handled = true`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CustomAction {
    /// Acting slot.
    pub actor: SlotRef,
    /// Action id.
    pub id: String,
    /// Opaque payload from the action input.
    pub payload: Vec<u8>,
    /// Set by the handler that executed the action.
    pub handled: bool,
}

/// Hook surface invoked by the battle core at named phases (§7.1, §7.4).
pub trait BattleHooks {
    /// Once, before [`BattleEvent::BattleStarted`].
    fn on_battle_start(&mut self, _ctx: &mut BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// At the beginning of each turn (after `TurnStarted`).
    fn on_turn_start(&mut self, _ctx: &mut BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// In [`BattlePhase::PreTurn`], after actions are locked and before ordering (formes,
    /// volatiles, item triggers).
    fn on_pre_turn(&mut self, _ctx: &mut BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// After the priority/speed sort; may reorder `order.entries`.
    fn on_action_order(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _order: &mut ActionOrder,
    ) -> Result<()> {
        Ok(())
    }

    /// Before a move is used: may change the move or target, or cancel it.
    fn on_before_move(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _req: &mut MoveRequest,
    ) -> Result<HookResult> {
        Ok(HookResult::Continue)
    }

    /// After the hit roll: may change `acc.hits`.
    fn on_accuracy(&mut self, _ctx: &mut BattleHookCtx<'_>, _acc: &mut AccuracyCalc) -> Result<()> {
        Ok(())
    }

    /// After the default damage pipeline: may push modifiers or override the final damage.
    fn on_damage_calc(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _dmg: &mut DamageCalc,
    ) -> Result<()> {
        Ok(())
    }

    /// Before damage is applied: may change the amount or cancel.
    fn on_damage_taken(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _hit: &mut DamageTaken,
    ) -> Result<HookResult> {
        Ok(HookResult::Continue)
    }

    /// After a move resolved (hit or miss).
    fn on_after_move(&mut self, _ctx: &mut BattleHookCtx<'_>, _res: &MoveResult) -> Result<()> {
        Ok(())
    }

    /// Before a status is applied: may change it or cancel (immunities).
    fn on_status_apply(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _req: &mut StatusApply,
    ) -> Result<HookResult> {
        Ok(HookResult::Continue)
    }

    /// Before a creature enters a slot; cancelling only affects voluntary switches.
    fn on_switch_in(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _sw: &mut SwitchIn,
    ) -> Result<HookResult> {
        Ok(HookResult::Continue)
    }

    /// Before a creature leaves a slot; cancel to trap it (voluntary switches only).
    fn on_switch_out(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _sw: &mut SwitchOut,
    ) -> Result<HookResult> {
        Ok(HookResult::Continue)
    }

    /// After a creature fainted.
    fn on_faint(&mut self, _ctx: &mut BattleHookCtx<'_>, _f: &FaintEvent) -> Result<()> {
        Ok(())
    }

    /// In [`BattlePhase::EndOfTurn`] (residuals, timers).
    fn on_end_of_turn(&mut self, _ctx: &mut BattleHookCtx<'_>) -> Result<()> {
        Ok(())
    }

    /// When the battle ends: may change rewards.
    fn on_battle_end(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _result: &mut BattleResult,
    ) -> Result<()> {
        Ok(())
    }

    /// When the UI asks for a slot's action menu ([`Battle::action_menu`](crate::Battle::action_menu)).
    fn on_action_menu(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _menu: &mut ActionMenu,
    ) -> Result<()> {
        Ok(())
    }

    /// Executes a [`BattleAction::Custom`](crate::BattleAction::Custom); the implementing
    /// handler sets `action.handled`.
    fn on_custom_action(
        &mut self,
        _ctx: &mut BattleHookCtx<'_>,
        _action: &mut CustomAction,
    ) -> Result<()> {
        Ok(())
    }
}

/// No-op hooks for tests and headless simulations.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullBattleHooks;

impl BattleHooks for NullBattleHooks {}

/// Records the name of every hook invoked, in call order (tests, debugging).
#[derive(Clone, Debug, Default)]
pub struct RecordingBattleHooks {
    /// Hook names in call order.
    pub log: Vec<&'static str>,
}

impl BattleHooks for RecordingBattleHooks {
    fn on_battle_start(&mut self, _: &mut BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_battle_start");
        Ok(())
    }

    fn on_turn_start(&mut self, _: &mut BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_turn_start");
        Ok(())
    }

    fn on_pre_turn(&mut self, _: &mut BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_pre_turn");
        Ok(())
    }

    fn on_action_order(&mut self, _: &mut BattleHookCtx<'_>, _: &mut ActionOrder) -> Result<()> {
        self.log.push("on_action_order");
        Ok(())
    }

    fn on_before_move(
        &mut self,
        _: &mut BattleHookCtx<'_>,
        _: &mut MoveRequest,
    ) -> Result<HookResult> {
        self.log.push("on_before_move");
        Ok(HookResult::Continue)
    }

    fn on_accuracy(&mut self, _: &mut BattleHookCtx<'_>, _: &mut AccuracyCalc) -> Result<()> {
        self.log.push("on_accuracy");
        Ok(())
    }

    fn on_damage_calc(&mut self, _: &mut BattleHookCtx<'_>, _: &mut DamageCalc) -> Result<()> {
        self.log.push("on_damage_calc");
        Ok(())
    }

    fn on_damage_taken(
        &mut self,
        _: &mut BattleHookCtx<'_>,
        _: &mut DamageTaken,
    ) -> Result<HookResult> {
        self.log.push("on_damage_taken");
        Ok(HookResult::Continue)
    }

    fn on_after_move(&mut self, _: &mut BattleHookCtx<'_>, _: &MoveResult) -> Result<()> {
        self.log.push("on_after_move");
        Ok(())
    }

    fn on_status_apply(
        &mut self,
        _: &mut BattleHookCtx<'_>,
        _: &mut StatusApply,
    ) -> Result<HookResult> {
        self.log.push("on_status_apply");
        Ok(HookResult::Continue)
    }

    fn on_switch_in(&mut self, _: &mut BattleHookCtx<'_>, _: &mut SwitchIn) -> Result<HookResult> {
        self.log.push("on_switch_in");
        Ok(HookResult::Continue)
    }

    fn on_switch_out(
        &mut self,
        _: &mut BattleHookCtx<'_>,
        _: &mut SwitchOut,
    ) -> Result<HookResult> {
        self.log.push("on_switch_out");
        Ok(HookResult::Continue)
    }

    fn on_faint(&mut self, _: &mut BattleHookCtx<'_>, _: &FaintEvent) -> Result<()> {
        self.log.push("on_faint");
        Ok(())
    }

    fn on_end_of_turn(&mut self, _: &mut BattleHookCtx<'_>) -> Result<()> {
        self.log.push("on_end_of_turn");
        Ok(())
    }

    fn on_battle_end(&mut self, _: &mut BattleHookCtx<'_>, _: &mut BattleResult) -> Result<()> {
        self.log.push("on_battle_end");
        Ok(())
    }

    fn on_action_menu(&mut self, _: &mut BattleHookCtx<'_>, _: &mut ActionMenu) -> Result<()> {
        self.log.push("on_action_menu");
        Ok(())
    }

    fn on_custom_action(&mut self, _: &mut BattleHookCtx<'_>, _: &mut CustomAction) -> Result<()> {
        self.log.push("on_custom_action");
        Ok(())
    }
}
