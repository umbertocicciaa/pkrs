//! Built-in trainer AI tiers (§7.5).
//!
//! The AI never touches battle state: it reads an [`AiView`] (the read-only [`BattleView`] plus
//! move data) and returns ordinary [`BattleAction`]s, which go through the same input path as
//! the player's. It rolls on its **own** RNG stream, derived from the battle's [`RngPolicy`]
//! per side, so the battle stream is untouched and a replay of the recorded inputs reproduces
//! the battle exactly without running any AI.

use pkrs_core::{GameRng, Pcg64Rng};
use pkrs_data::{AiProfile, AiTier, BattlePack, MoveCategory, MoveDef};

use crate::action::{BattleAction, SideInput};
use crate::battle::Battle;
use crate::damage::{effective_defense, effective_offense, gen3_base_damage};
use crate::hooks::{BattleView, CreatureView, SideView};
use crate::model::{PartyIndex, SideId, SlotIndex};
use crate::phase::BattlePhase;
use crate::rng::RngPolicy;

/// What an AI may look at when deciding.
#[derive(Clone, Copy)]
pub struct AiView<'a> {
    /// Field snapshot.
    pub battle: &'a BattleView,
    /// The side the AI plays.
    pub side: SideId,
    /// Move and type data.
    pub pack: &'a BattlePack,
}

impl<'a> AiView<'a> {
    /// The AI's own side.
    #[must_use]
    pub fn own(&self) -> &'a SideView {
        self.battle.side(self.side)
    }

    /// The opposing side.
    #[must_use]
    pub fn foe(&self) -> &'a SideView {
        self.battle.side(self.side.other())
    }

    /// Living opposing actives, by slot.
    #[must_use]
    pub fn foes(&self) -> Vec<(SlotIndex, &'a CreatureView)> {
        let foe = self.foe();
        (0..self.battle.side_size)
            .filter_map(|s| {
                foe.active_creature(s)
                    .filter(|c| !c.fainted())
                    .map(|c| (s, c))
            })
            .collect()
    }

    /// Moves `creature` can use right now (known to the pack, PP left), with their slot.
    #[must_use]
    pub fn usable_moves(&self, creature: &CreatureView) -> Vec<(u8, &'a MoveDef)> {
        creature
            .moves
            .iter()
            .enumerate()
            .filter_map(|(i, m)| {
                let m = m.as_ref()?;
                let def = self.pack.move_by_id(&m.id)?;
                (m.pp > 0).then_some((i as u8, def))
            })
            .collect()
    }

    /// Expected damage of `mv` from `attacker` to `defender`: the Gen-3 formula with stages,
    /// STAB and type effectiveness, an average random roll and no critical hit.
    #[must_use]
    pub fn estimate_damage(
        &self,
        attacker: &CreatureView,
        mv: &MoveDef,
        defender: &CreatureView,
    ) -> u32 {
        if mv.category == MoveCategory::Status || mv.power == 0 {
            return 0;
        }
        let offense = effective_offense(attacker.stats.attack, &attacker.stages, mv.category);
        let defense = effective_defense(defender.stats.defense, &defender.stages, mv.category);
        let base = gen3_base_damage(attacker.level, mv.power, offense, defense);
        let stab = if attacker.types.contains(&mv.type_id) {
            1.5
        } else {
            1.0
        };
        let types = self
            .pack
            .type_chart
            .combined_multiplier(&mv.type_id, &defender.types);
        (f64::from(base) * stab * types * 0.925).floor() as u32
    }

    /// Best expected damage of `attacker` against `defender`, in permille of the defender's
    /// max HP (capped at 1000).
    #[must_use]
    pub fn best_damage_permille(&self, attacker: &CreatureView, defender: &CreatureView) -> u32 {
        let max_hp = defender.stats.max_hp.max(1);
        self.usable_moves(attacker)
            .iter()
            .map(|(_, mv)| self.estimate_damage(attacker, mv, defender))
            .max()
            .map_or(0, |d| {
                (u64::from(d) * 1000 / u64::from(max_hp)).min(1000) as u32
            })
    }
}

/// Decides actions for one side (§7.5). Implemented by the built-in tiers; Lua AI profiles
/// implement it in `pkrs-script`.
pub trait BattleAi {
    /// Action for active `slot` during `SelectActions`.
    fn choose_action(&mut self, view: &AiView<'_>, slot: SlotIndex) -> BattleAction;

    /// Party index to send into `slot` after a faint (`Replace`).
    fn choose_replacement(&mut self, view: &AiView<'_>, slot: SlotIndex) -> PartyIndex;
}

/// The input `ai` gives for `side` at the battle's current phase, or `None` if `side` has
/// nothing to choose right now.
pub fn ai_side_input(ai: &mut dyn BattleAi, battle: &Battle, side: SideId) -> Option<SideInput> {
    let snapshot = battle.view();
    let view = AiView {
        battle: &snapshot,
        side,
        pack: &battle.resources().pack,
    };
    match battle.phase() {
        BattlePhase::SelectActions => {
            let mut chosen_switches = Vec::new();
            let slots = (0..battle.config().side_size)
                .map(|slot| {
                    let action = ai.choose_action(&view, slot);
                    match action {
                        BattleAction::Switch { to_party }
                            if chosen_switches.contains(&to_party) =>
                        {
                            fallback_move(&view, slot)
                        }
                        BattleAction::Switch { to_party } => {
                            chosen_switches.push(to_party);
                            action
                        }
                        other => other,
                    }
                })
                .collect();
            Some(SideInput { slots })
        }
        BattlePhase::Replace { side: s, slot } if s == side => Some(SideInput {
            slots: vec![BattleAction::Switch {
                to_party: ai.choose_replacement(&view, slot),
            }],
        }),
        _ => None,
    }
}

fn fallback_move(view: &AiView<'_>, slot: SlotIndex) -> BattleAction {
    let target = view.foes().first().map_or(0, |(s, _)| *s);
    let move_slot = view
        .own()
        .active_creature(slot)
        .and_then(|c| view.usable_moves(c).first().map(|(i, _)| *i))
        .unwrap_or(0);
    BattleAction::Fight {
        move_slot,
        target_side: view.side.other(),
        target_slot: target,
    }
}

/// A built-in AI: [`AiTier::Random`], [`AiTier::Basic`] or [`AiTier::Smart`], tuned by an
/// [`AiProfile`].
#[derive(Clone, Debug)]
pub struct TrainerAi {
    tier: AiTier,
    aggression: u32,
    switch: u32,
    predict: u32,
    allow_items: bool,
    heal_below_percent: u8,
    items: Vec<String>,
    rng: Pcg64Rng,
}

impl TrainerAi {
    /// AI for `profile` with the trainer's `items`, rolling on a stream seeded with `seed`.
    #[must_use]
    pub fn new(profile: &AiProfile, items: Vec<String>, seed: u64) -> Self {
        let (aggression, switch, predict) = profile.weights.permille();
        Self {
            tier: profile.tier,
            aggression: u32::from(aggression),
            switch: u32::from(switch),
            predict: u32::from(predict),
            allow_items: profile.allow_items,
            heal_below_percent: profile.heal_below_percent,
            items,
            rng: Pcg64Rng::from_seed(seed),
        }
    }

    /// AI for `side` of a battle created with `policy` (stream from [`RngPolicy::ai_seed`]).
    #[must_use]
    pub fn for_battle(
        profile: &AiProfile,
        items: Vec<String>,
        policy: &RngPolicy,
        side: SideId,
    ) -> Self {
        Self::new(profile, items, policy.ai_seed(side))
    }

    /// The tier.
    #[must_use]
    pub fn tier(&self) -> AiTier {
        self.tier
    }

    /// Items the AI has not used yet.
    #[must_use]
    pub fn items(&self) -> &[String] {
        &self.items
    }

    /// Index of the best of `scores` (higher is better); ties broken on the AI stream.
    fn pick_best(&mut self, scores: &[i64]) -> Option<usize> {
        let best = *scores.iter().max()?;
        let tied: Vec<usize> = (0..scores.len()).filter(|&i| scores[i] == best).collect();
        let pick = self.rng.gen_range_u32(tied.len() as u32) as usize;
        tied.get(pick).copied()
    }

    fn try_item(
        &mut self,
        view: &AiView<'_>,
        me: &CreatureView,
        slot: SlotIndex,
    ) -> Option<BattleAction> {
        if !self.allow_items || self.items.is_empty() || self.tier == AiTier::Random {
            return None;
        }
        let low = u64::from(me.hp) * 100
            <= u64::from(me.stats.max_hp) * u64::from(self.heal_below_percent);
        (low && !me.fainted()).then(|| BattleAction::Item {
            item_id: self.items.remove(0),
            target_side: view.side,
            target_slot: slot,
        })
    }

    fn random_action(&mut self, view: &AiView<'_>, me: &CreatureView) -> BattleAction {
        let moves = view.usable_moves(me);
        let foes = view.foes();
        let move_slot = match moves.len() {
            0 => 0,
            n => moves[self.rng.gen_range_u32(n as u32) as usize].0,
        };
        let target = match foes.len() {
            0 => 0,
            n => foes[self.rng.gen_range_u32(n as u32) as usize].0,
        };
        BattleAction::Fight {
            move_slot,
            target_side: view.side.other(),
            target_slot: target,
        }
    }

    /// Basic: power × STAB × type effectiveness; status moves only as a last resort.
    fn basic_score(view: &AiView<'_>, me: &CreatureView, mv: &MoveDef, foe: &CreatureView) -> i64 {
        if mv.category == MoveCategory::Status || mv.power == 0 {
            return 1;
        }
        let stab = if me.types.contains(&mv.type_id) {
            1.5
        } else {
            1.0
        };
        let types = view
            .pack
            .type_chart
            .combined_multiplier(&mv.type_id, &foe.types);
        (f64::from(mv.power) * stab * types * 100.0).floor() as i64
    }

    /// Smart: expected damage as permille of the target's HP; a KO beats everything, higher
    /// priority KOs first; status moves are worth more to less aggressive profiles.
    fn smart_score(
        &self,
        view: &AiView<'_>,
        me: &CreatureView,
        mv: &MoveDef,
        foe: &CreatureView,
    ) -> i64 {
        if mv.category == MoveCategory::Status || mv.power == 0 {
            return i64::from((1000 - self.aggression) / 10);
        }
        let dmg = view.estimate_damage(me, mv, foe);
        if dmg >= foe.hp {
            return 1_000_000 + i64::from(mv.priority) * 10_000 + i64::from(dmg.min(9_999));
        }
        let max_hp = u64::from(foe.stats.max_hp.max(1));
        (u64::from(dmg) * 1000 / max_hp) as i64
    }

    fn best_attack(&mut self, view: &AiView<'_>, me: &CreatureView) -> Option<(BattleAction, i64)> {
        let mut options = Vec::new();
        let mut scores = Vec::new();
        for (move_slot, mv) in view.usable_moves(me) {
            for (target, foe) in view.foes() {
                let score = match self.tier {
                    AiTier::Smart => self.smart_score(view, me, mv, foe),
                    _ => Self::basic_score(view, me, mv, foe),
                };
                options.push((move_slot, target));
                scores.push(score);
            }
        }
        let i = self.pick_best(&scores)?;
        let (move_slot, target) = options[i];
        Some((
            BattleAction::Fight {
                move_slot,
                target_side: view.side.other(),
                target_slot: target,
            },
            scores[i],
        ))
    }

    /// Smart matchup of `c` against the living foes: best offense minus the foes' best threat,
    /// weighted by `predict` (permille scale).
    fn matchup(&self, view: &AiView<'_>, c: &CreatureView) -> i64 {
        view.foes()
            .iter()
            .map(|(_, foe)| {
                let offense = i64::from(view.best_damage_permille(c, foe));
                let threat = i64::from(view.best_damage_permille(foe, c));
                offense - threat * i64::from(self.predict) / 1000
            })
            .max()
            .unwrap_or(0)
    }

    fn best_bench(&mut self, view: &AiView<'_>) -> Option<(PartyIndex, i64)> {
        let bench: Vec<&CreatureView> = view.own().bench().collect();
        let scores: Vec<i64> = bench
            .iter()
            .map(|c| match self.tier {
                AiTier::Smart => self.matchup(view, c),
                AiTier::Basic => view
                    .foes()
                    .iter()
                    .flat_map(|(_, foe)| {
                        view.usable_moves(c)
                            .into_iter()
                            .map(move |(_, mv)| Self::basic_score(view, c, mv, foe))
                    })
                    .max()
                    .unwrap_or(0),
                AiTier::Random => 0,
            })
            .collect();
        let i = self.pick_best(&scores)?;
        Some((bench[i].party_index, scores[i]))
    }

    /// Smart switching: leave a walled matchup (best hit ≤ 15% of the foe's HP) for a bench
    /// creature that matches up clearly better, with probability `switch` permille.
    fn consider_switch(
        &mut self,
        view: &AiView<'_>,
        me: &CreatureView,
        attack_score: i64,
    ) -> Option<BattleAction> {
        if self.switch == 0 || attack_score >= 1_000_000 {
            return None;
        }
        let walled = view
            .foes()
            .iter()
            .all(|(_, foe)| view.best_damage_permille(me, foe) <= 150);
        if !walled {
            return None;
        }
        let current = self.matchup(view, me);
        let (to_party, best) = self.best_bench(view)?;
        let margin = 100;
        if best <= current + margin {
            return None;
        }
        (self.rng.gen_range_u32(1000) < self.switch).then_some(BattleAction::Switch { to_party })
    }
}

impl BattleAi for TrainerAi {
    fn choose_action(&mut self, view: &AiView<'_>, slot: SlotIndex) -> BattleAction {
        let Some(me) = view.own().active_creature(slot) else {
            return fallback_move(view, slot);
        };
        if self.tier == AiTier::Random {
            return self.random_action(view, me);
        }
        if let Some(item) = self.try_item(view, me, slot) {
            return item;
        }
        let Some((attack, score)) = self.best_attack(view, me) else {
            return fallback_move(view, slot);
        };
        if self.tier == AiTier::Smart {
            if let Some(switch) = self.consider_switch(view, me, score) {
                return switch;
            }
        }
        attack
    }

    fn choose_replacement(&mut self, view: &AiView<'_>, _slot: SlotIndex) -> PartyIndex {
        let bench: Vec<PartyIndex> = view.own().bench().map(|c| c.party_index).collect();
        if self.tier == AiTier::Random {
            return match bench.len() {
                0 => 0,
                n => bench[self.rng.gen_range_u32(n as u32) as usize],
            };
        }
        self.best_bench(view)
            .map(|(idx, _)| idx)
            .or_else(|| bench.first().copied())
            .unwrap_or(0)
    }
}
