//! Event → presentation mapping (§7.2: the UI plays events back).
//!
//! The battle logic produces a whole turn of [`BattleEvent`]s at once. A [`Presenter`] turns
//! each event, in order, into [`Cue`]s the battle screen plays one after another: a text box
//! (battle text goes through the Dialogue component), an HP bar animation, a faint, a
//! switch-out or a switch-in. The presenter keeps its own [`Roster`] (names, HP, who is
//! active) that runs *ahead* of what is on screen, so a cue always carries the values it
//! animates between and playback never queries the battle.

use pkrs_battle::{BattleEvent, BattleOutcome, BattleSetup, PartyIndex, SideId, SlotIndex};
use pkrs_data::StatId;

use super::content::{BattleKind, display_name};

/// One presentation step of the playback queue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cue {
    /// Show a battle message and wait for Confirm.
    Text(String),
    /// Animate the HP bar of `party` on `side` from `from` to `to`.
    Hp {
        /// Side.
        side: SideId,
        /// Party member.
        party: PartyIndex,
        /// HP before.
        from: u32,
        /// HP after.
        to: u32,
    },
    /// The creature in `slot` faints (its sprite drops out).
    Faint {
        /// Side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
    },
    /// The creature in `slot` is withdrawn.
    SwitchOut {
        /// Side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
    },
    /// `party` is sent into `slot`.
    SwitchIn {
        /// Side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
        /// Party member sent out.
        party: PartyIndex,
    },
    /// Show or clear a battle FX on a creature (`BattleEvent::Fx` from `ctx.play_fx` /
    /// `ctx.clear_fx`, §9.5).
    Fx {
        /// Side.
        side: SideId,
        /// Active slot.
        slot: SlotIndex,
        /// FX asset id (`plugin:asset`); `None` clears the effect on this slot.
        id: Option<String>,
    },
}

/// One party member as the UI knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// Display name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Current HP.
    pub hp: u32,
    /// Maximum HP.
    pub max_hp: u32,
}

/// One side as the UI knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SideRoster {
    /// Trainer or player label.
    pub name: String,
    /// Whole party.
    pub members: Vec<Member>,
    /// Party member in each active slot.
    pub active: Vec<Option<PartyIndex>>,
}

impl SideRoster {
    /// Party member in `slot`.
    #[must_use]
    pub fn active_member(&self, slot: SlotIndex) -> Option<&Member> {
        let index = (*self.active.get(usize::from(slot))?)?;
        self.members.get(usize::from(index))
    }

    fn member_mut(&mut self, party: PartyIndex) -> Option<&mut Member> {
        self.members.get_mut(usize::from(party))
    }

    fn active_index(&self, slot: SlotIndex) -> Option<PartyIndex> {
        *self.active.get(usize::from(slot))?
    }
}

/// Names, HP and active slots of both sides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Roster {
    /// `[player, opponent]`.
    pub sides: [SideRoster; 2],
}

impl Roster {
    /// The roster at the start of `setup`.
    #[must_use]
    pub fn from_setup(setup: &BattleSetup) -> Self {
        let side = |s: &pkrs_battle::BattleSideSetup| SideRoster {
            name: s.name.clone(),
            members: s
                .party
                .iter()
                .map(|c| Member {
                    name: c.nickname.clone(),
                    level: c.level,
                    hp: c.stats.max_hp,
                    max_hp: c.stats.max_hp,
                })
                .collect(),
            active: s.leading.clone(),
        };
        Self {
            sides: [side(&setup.player), side(&setup.opponent)],
        }
    }

    /// `side`'s roster.
    #[must_use]
    pub fn side(&self, side: SideId) -> &SideRoster {
        match side {
            SideId::Player => &self.sides[0],
            SideId::Opponent => &self.sides[1],
        }
    }

    fn side_mut(&mut self, side: SideId) -> &mut SideRoster {
        match side {
            SideId::Player => &mut self.sides[0],
            SideId::Opponent => &mut self.sides[1],
        }
    }

    /// Sets the HP of `party` on `side`.
    pub fn set_hp(&mut self, side: SideId, party: PartyIndex, hp: u32) {
        if let Some(m) = self.side_mut(side).member_mut(party) {
            m.hp = hp.min(m.max_hp);
        }
    }

    /// Applies the end state of `cue` (HP bars jump to their target).
    pub fn apply(&mut self, cue: &Cue) {
        match *cue {
            Cue::Hp {
                side, party, to, ..
            } => self.set_hp(side, party, to),
            Cue::SwitchIn { side, slot, party } => {
                if let Some(a) = self.side_mut(side).active.get_mut(usize::from(slot)) {
                    *a = Some(party);
                }
            }
            Cue::Text(_) | Cue::Faint { .. } | Cue::SwitchOut { .. } | Cue::Fx { .. } => {}
        }
    }
}

/// Maps [`BattleEvent`]s to [`Cue`]s (§7.2).
#[derive(Clone, Debug)]
pub struct Presenter {
    kind: BattleKind,
    roster: Roster,
    /// Last known stat stage per `(side, slot, stat)`, to tell rises from drops.
    stages: Vec<(SideId, SlotIndex, StatId, i8)>,
}

impl Presenter {
    /// A presenter for a battle of `kind` starting from `setup`.
    #[must_use]
    pub fn new(kind: BattleKind, setup: &BattleSetup) -> Self {
        Self {
            kind,
            roster: Roster::from_setup(setup),
            stages: Vec::new(),
        }
    }

    /// The roster as of the last mapped event.
    #[must_use]
    pub fn roster(&self) -> &Roster {
        &self.roster
    }

    /// Cues for a whole batch of events, in order.
    pub fn present_all(&mut self, events: &[BattleEvent]) -> Vec<Cue> {
        events.iter().flat_map(|e| self.present(e)).collect()
    }

    /// Cues for one event. Bookkeeping events (turn markers, order, hook traces, plugin
    /// state) have none.
    pub fn present(&mut self, event: &BattleEvent) -> Vec<Cue> {
        match event {
            BattleEvent::BattleStarted => self.intro(),
            BattleEvent::MoveUsed {
                user_side,
                user_slot,
                move_id,
                ..
            } => vec![Cue::Text(format!(
                "{} used {}!",
                self.name(*user_side, *user_slot),
                display_name(move_id)
            ))],
            BattleEvent::Damage {
                target_side,
                target_slot,
                remaining_hp,
                ..
            } => self.hp_to(*target_side, *target_slot, |_| *remaining_hp),
            BattleEvent::Heal { side, slot, amount } => {
                let mut cues = self.hp_to(*side, *slot, |hp| hp.saturating_add(*amount));
                cues.push(Cue::Text(format!(
                    "{} regained health!",
                    self.name(*side, *slot)
                )));
                cues
            }
            BattleEvent::Recoil { side, slot, amount } => {
                let mut cues = self.hp_to(*side, *slot, |hp| hp.saturating_sub(*amount));
                cues.push(Cue::Text(format!(
                    "{} is hit with recoil!",
                    self.name(*side, *slot)
                )));
                cues
            }
            BattleEvent::Missed { .. } => vec![Cue::Text("The attack missed!".into())],
            BattleEvent::StatChanged {
                side,
                slot,
                stat,
                stages,
            } => self.stat_changed(*side, *slot, *stat, *stages),
            BattleEvent::StatusApplied {
                target_side,
                target_slot,
                status,
            } => vec![Cue::Text(format!(
                "{} is now {}!",
                self.name(*target_side, *target_slot),
                display_name(status).to_lowercase()
            ))],
            BattleEvent::StatusCured {
                target_side,
                target_slot,
                status,
            } => vec![Cue::Text(format!(
                "{} is no longer {}.",
                self.name(*target_side, *target_slot),
                display_name(status).to_lowercase()
            ))],
            BattleEvent::VolatileEnded {
                target_side,
                target_slot,
                volatile,
            } => vec![Cue::Text(format!(
                "{}'s {} ended.",
                self.name(*target_side, *target_slot),
                display_name(volatile).to_lowercase()
            ))],
            BattleEvent::WeatherSet { weather } => vec![Cue::Text(format!(
                "The weather turned to {}.",
                display_name(weather).to_lowercase()
            ))],
            BattleEvent::WeatherEnded => vec![Cue::Text("The weather cleared.".into())],
            BattleEvent::Fainted { side, slot, .. } => {
                let name = self.name(*side, *slot);
                vec![
                    Cue::Faint {
                        side: *side,
                        slot: *slot,
                    },
                    Cue::Text(format!("{name} fainted!")),
                ]
            }
            BattleEvent::Switched {
                side,
                slot,
                to_party,
            } => self.switched(*side, *slot, *to_party),
            BattleEvent::Ran { side } => match side {
                SideId::Player => vec![Cue::Text("Got away safely!".into())],
                SideId::Opponent => vec![Cue::Text(format!(
                    "{} fled!",
                    self.name(SideId::Opponent, 0)
                ))],
            },
            BattleEvent::Message { key } => vec![Cue::Text(message_text(key))],
            BattleEvent::Fx { side, slot, id } => vec![Cue::Fx {
                side: *side,
                slot: *slot,
                id: id.clone(),
            }],
            BattleEvent::BattleEnded { outcome } => self.ended(*outcome),
            BattleEvent::MoveCancelled {
                user_side,
                user_slot,
                move_id,
                reason,
            } => vec![Cue::Text(reason.as_deref().map_or_else(
                || {
                    format!(
                        "{} couldn't use {}!",
                        self.name(*user_side, *user_slot),
                        display_name(move_id)
                    )
                },
                message_text,
            ))],
            BattleEvent::DamagePrevented {
                target_side,
                target_slot,
                reason,
            } => vec![Cue::Text(reason.as_deref().map_or_else(
                || {
                    format!(
                        "{} protected itself!",
                        self.name(*target_side, *target_slot)
                    )
                },
                message_text,
            ))],
            BattleEvent::StatusBlocked {
                target_side,
                target_slot,
                reason,
                ..
            } => vec![Cue::Text(reason.as_deref().map_or_else(
                || {
                    format!(
                        "It doesn't affect {}...",
                        self.name(*target_side, *target_slot)
                    )
                },
                message_text,
            ))],
            BattleEvent::SwitchCancelled { side, slot, reason } => {
                vec![Cue::Text(reason.as_deref().map_or_else(
                    || format!("{} can't be switched out!", self.name(*side, *slot)),
                    message_text,
                ))]
            }
            BattleEvent::CustomActionUsed { handled: false, .. } => {
                vec![Cue::Text("But nothing happened!".into())]
            }
            _ => Vec::new(),
        }
    }

    /// Opening texts (mapped from [`BattleEvent::BattleStarted`]).
    fn intro(&self) -> Vec<Cue> {
        let foe = self.member_name(SideId::Opponent, 0);
        let mine = self.member_name(SideId::Player, 0);
        let mut cues = match &self.kind {
            BattleKind::Wild => vec![Cue::Text(format!("A wild {foe} appeared!"))],
            BattleKind::Trainer { name } => vec![
                Cue::Text(format!("{name} wants to battle!")),
                Cue::Text(format!("{name} sent out {foe}!")),
            ],
        };
        cues.push(Cue::Text(format!("Go! {mine}!")));
        cues
    }

    fn ended(&self, outcome: BattleOutcome) -> Vec<Cue> {
        let player = &self.roster.side(SideId::Player).name;
        match (outcome, &self.kind) {
            (BattleOutcome::PlayerWin, BattleKind::Trainer { name }) => {
                vec![Cue::Text(format!("{player} defeated {name}!"))]
            }
            (BattleOutcome::OpponentWin, _) => vec![
                Cue::Text(format!("{player} is out of usable creatures!")),
                Cue::Text(format!("{player} blacked out!")),
            ],
            _ => Vec::new(),
        }
    }

    fn switched(&mut self, side: SideId, slot: SlotIndex, to_party: PartyIndex) -> Vec<Cue> {
        let mut cues = Vec::new();
        let trainer = match &self.kind {
            BattleKind::Trainer { name } => Some(name.clone()),
            BattleKind::Wild => None,
        };
        let previous = self.roster.side(side).active_index(slot);
        if let Some(old) = previous.and_then(|i| self.roster.side(side).members.get(usize::from(i)))
        {
            if old.hp > 0 {
                let text = match (side, &trainer) {
                    (SideId::Player, _) => format!("{}, come back!", old.name),
                    (SideId::Opponent, Some(t)) => format!("{t} withdrew {}!", old.name),
                    (SideId::Opponent, None) => format!("{} withdrew!", old.name),
                };
                cues.push(Cue::Text(text));
                cues.push(Cue::SwitchOut { side, slot });
            }
        }
        let cue = Cue::SwitchIn {
            side,
            slot,
            party: to_party,
        };
        self.roster.apply(&cue);
        self.stages
            .retain(|(s, sl, _, _)| (*s, *sl) != (side, slot));
        cues.push(cue);
        let new = self.member_name(side, slot);
        cues.push(Cue::Text(match (side, &trainer) {
            (SideId::Player, _) => format!("Go! {new}!"),
            (SideId::Opponent, Some(t)) => format!("{t} sent out {new}!"),
            (SideId::Opponent, None) => format!("{new} appeared!"),
        }));
        cues
    }

    fn stat_changed(
        &mut self,
        side: SideId,
        slot: SlotIndex,
        stat: StatId,
        stages: i8,
    ) -> Vec<Cue> {
        let entry = self
            .stages
            .iter_mut()
            .find(|(s, sl, st, _)| (*s, *sl, *st) == (side, slot, stat));
        let before = match entry {
            Some(e) => std::mem::replace(&mut e.3, stages),
            None => {
                self.stages.push((side, slot, stat, stages));
                0
            }
        };
        let what = format!("{}'s {}", self.name(side, slot), stat_name(stat));
        let text = match stages.cmp(&before) {
            std::cmp::Ordering::Greater => format!("{what} rose!"),
            std::cmp::Ordering::Less => format!("{what} fell!"),
            std::cmp::Ordering::Equal if stages > 0 => format!("{what} won't go any higher!"),
            std::cmp::Ordering::Equal => format!("{what} won't go any lower!"),
        };
        vec![Cue::Text(text)]
    }

    /// An HP cue for the creature in `slot`, moving its HP to `to(hp)`.
    fn hp_to(&mut self, side: SideId, slot: SlotIndex, to: impl FnOnce(u32) -> u32) -> Vec<Cue> {
        let roster = self.roster.side(side);
        let Some(party) = roster.active_index(slot) else {
            return Vec::new();
        };
        let Some(member) = roster.members.get(usize::from(party)) else {
            return Vec::new();
        };
        let from = member.hp;
        let to = to(from).min(member.max_hp);
        if to == from {
            return Vec::new();
        }
        let cue = Cue::Hp {
            side,
            party,
            from,
            to,
        };
        self.roster.apply(&cue);
        vec![cue]
    }

    fn member_name(&self, side: SideId, slot: SlotIndex) -> String {
        self.roster
            .side(side)
            .active_member(slot)
            .map_or_else(|| "???".to_owned(), |m| m.name.clone())
    }

    /// How texts name the creature in `slot`: the player's by name, the foe's as
    /// "Wild X" or "Foe X".
    fn name(&self, side: SideId, slot: SlotIndex) -> String {
        let name = self.member_name(side, slot);
        match (side, &self.kind) {
            (SideId::Player, _) => name,
            (SideId::Opponent, BattleKind::Wild) => format!("Wild {name}"),
            (SideId::Opponent, BattleKind::Trainer { .. }) => format!("Foe {name}"),
        }
    }
}

fn stat_name(stat: StatId) -> &'static str {
    match stat {
        StatId::Attack => "attack",
        StatId::Defense => "defense",
        StatId::Speed => "speed",
        StatId::Accuracy => "accuracy",
        StatId::Evasion => "evasiveness",
    }
}

/// Text of a [`BattleEvent::Message`] key (or a hook's reason key). Known engine keys have
/// built-in English texts; `text:<key>` and unknown keys are shown readable until localized
/// text data lands (§8).
#[must_use]
pub fn message_text(key: &str) -> String {
    match key {
        "status:cant_move" => "It can't move!".into(),
        "protect" => "It protected itself!".into(),
        "switch_out" => "It was forced out!".into(),
        _ => {
            if let Some(n) = key.strip_prefix("multi_hit:") {
                format!("Hit {} more time(s)!", n)
            } else if let Some(item) = key.strip_prefix("item_used:") {
                format!("Used {}!", display_name(item))
            } else if let Some(source) = key.strip_prefix("residual:") {
                format!("It is hurt by {}!", display_name(source).to_lowercase())
            } else {
                key.strip_prefix("text:").unwrap_or(key).to_owned()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_battle::battle_testkit;

    fn trainer() -> Presenter {
        Presenter::new(
            BattleKind::Trainer {
                name: "Rival".into(),
            },
            &battle_testkit::wild_1v1(),
        )
    }

    fn texts(cues: &[Cue]) -> Vec<&str> {
        cues.iter()
            .filter_map(|c| match c {
                Cue::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn battle_start_maps_to_intro_texts() {
        let mut wild = Presenter::new(BattleKind::Wild, &battle_testkit::wild_1v1());
        let cues = wild.present(&BattleEvent::BattleStarted);
        assert_eq!(texts(&cues), ["A wild foe appeared!", "Go! hero!"]);
        let cues = trainer().present(&BattleEvent::BattleStarted);
        assert_eq!(
            texts(&cues),
            ["Rival wants to battle!", "Rival sent out foe!", "Go! hero!"]
        );
    }

    #[test]
    fn move_and_damage_map_to_text_then_hp_animation() {
        let mut p = trainer();
        let cues = p.present_all(&[
            BattleEvent::TurnStarted { turn: 1 },
            BattleEvent::MoveUsed {
                user_side: SideId::Player,
                user_slot: 0,
                move_id: "tackle".into(),
                target_side: SideId::Opponent,
                target_slot: 0,
            },
            BattleEvent::Damage {
                target_side: SideId::Opponent,
                target_slot: 0,
                amount: 12,
                remaining_hp: 18,
            },
            BattleEvent::HookTrace {
                hook: "on_after_move".into(),
            },
        ]);
        assert_eq!(
            cues,
            [
                Cue::Text("hero used Tackle!".into()),
                Cue::Hp {
                    side: SideId::Opponent,
                    party: 0,
                    from: 30,
                    to: 18
                },
            ]
        );
        assert_eq!(p.roster().side(SideId::Opponent).members[0].hp, 18);
        // Heal and recoil animate from the presenter's own HP and are capped.
        let cues = p.present(&BattleEvent::Heal {
            side: SideId::Opponent,
            slot: 0,
            amount: 100,
        });
        assert_eq!(
            cues[0],
            Cue::Hp {
                side: SideId::Opponent,
                party: 0,
                from: 18,
                to: 30
            }
        );
        assert_eq!(texts(&cues), ["Foe foe regained health!"]);
        let cues = p.present(&BattleEvent::Recoil {
            side: SideId::Player,
            slot: 0,
            amount: 5,
        });
        assert_eq!(
            cues[0],
            Cue::Hp {
                side: SideId::Player,
                party: 0,
                from: 40,
                to: 35
            }
        );
    }

    #[test]
    fn faint_switch_and_end_map_to_animations_and_texts() {
        let mut setup = battle_testkit::wild_1v1();
        let mut second = setup.opponent.party[0].clone();
        second.nickname = "second".into();
        setup.opponent.party.push(second);
        let mut p = Presenter::new(
            BattleKind::Trainer {
                name: "Rival".into(),
            },
            &setup,
        );
        let cues = p.present_all(&[
            BattleEvent::Damage {
                target_side: SideId::Opponent,
                target_slot: 0,
                amount: 30,
                remaining_hp: 0,
            },
            BattleEvent::Fainted {
                side: SideId::Opponent,
                slot: 0,
                party_index: 0,
            },
            BattleEvent::Switched {
                side: SideId::Opponent,
                slot: 0,
                to_party: 1,
            },
        ]);
        assert_eq!(
            cues[1..],
            [
                Cue::Faint {
                    side: SideId::Opponent,
                    slot: 0
                },
                Cue::Text("Foe foe fainted!".into()),
                // The fainted creature is not withdrawn again.
                Cue::SwitchIn {
                    side: SideId::Opponent,
                    slot: 0,
                    party: 1
                },
                Cue::Text("Rival sent out second!".into()),
            ]
        );
        // A voluntary switch withdraws the healthy creature first.
        let cues = p.present(&BattleEvent::Switched {
            side: SideId::Opponent,
            slot: 0,
            to_party: 0,
        });
        assert_eq!(
            texts(&cues),
            ["Rival withdrew second!", "Rival sent out foe!"]
        );
        assert!(cues.contains(&Cue::SwitchOut {
            side: SideId::Opponent,
            slot: 0
        }));
        let end = p.present(&BattleEvent::BattleEnded {
            outcome: BattleOutcome::PlayerWin,
        });
        assert_eq!(texts(&end), ["Player defeated Rival!"]);
        let lost = p.present(&BattleEvent::BattleEnded {
            outcome: BattleOutcome::OpponentWin,
        });
        assert_eq!(lost.len(), 2);
    }

    #[test]
    fn fx_events_map_to_fx_cues_on_their_own_slot() {
        let mut p = trainer();
        let show = p.present(&BattleEvent::Fx {
            side: SideId::Opponent,
            slot: 1,
            id: Some("test:glow".into()),
        });
        assert_eq!(
            show,
            vec![Cue::Fx {
                side: SideId::Opponent,
                slot: 1,
                id: Some("test:glow".into()),
            }]
        );
        let clear = p.present(&BattleEvent::Fx {
            side: SideId::Opponent,
            slot: 1,
            id: None,
        });
        assert_eq!(
            clear,
            vec![Cue::Fx {
                side: SideId::Opponent,
                slot: 1,
                id: None,
            }]
        );
    }

    #[test]
    fn fx_like_messages_and_ended_volatiles_are_plain_text() {
        let mut p = trainer();
        let cues = p.present(&BattleEvent::Message {
            key: "fx:test:glow".into(),
        });
        assert!(matches!(cues.as_slice(), [Cue::Text(_)]), "{cues:?}");
        let cues = p.present(&BattleEvent::VolatileEnded {
            target_side: SideId::Player,
            target_slot: 0,
            volatile: "glowing".into(),
        });
        assert!(matches!(cues.as_slice(), [Cue::Text(_)]), "{cues:?}");
    }

    #[test]
    fn stats_statuses_and_messages_have_texts() {
        let mut p = Presenter::new(BattleKind::Wild, &battle_testkit::wild_1v1());
        let stat = |stages| BattleEvent::StatChanged {
            side: SideId::Opponent,
            slot: 0,
            stat: StatId::Attack,
            stages,
        };
        assert_eq!(texts(&p.present(&stat(-1))), ["Wild foe's attack fell!"]);
        assert_eq!(texts(&p.present(&stat(1))), ["Wild foe's attack rose!"]);
        assert_eq!(
            texts(&p.present(&stat(1))),
            ["Wild foe's attack won't go any higher!"]
        );
        let status = p.present(&BattleEvent::StatusApplied {
            target_side: SideId::Player,
            target_slot: 0,
            status: "burn".into(),
        });
        assert_eq!(texts(&status), ["hero is now burn!"]);
        assert_eq!(message_text("status:cant_move"), "It can't move!");
        assert_eq!(message_text("item_used:potion"), "Used Potion!");
        assert_eq!(message_text("text:resonance.ready"), "resonance.ready");
        let ran = p.present(&BattleEvent::Ran {
            side: SideId::Player,
        });
        assert_eq!(texts(&ran), ["Got away safely!"]);
        let cancelled = p.present(&BattleEvent::MoveCancelled {
            user_side: SideId::Player,
            user_slot: 0,
            move_id: "tackle".into(),
            reason: Some("status:cant_move".into()),
        });
        assert_eq!(texts(&cancelled), ["It can't move!"]);
        assert!(p.present(&BattleEvent::ActionsLocked).is_empty());
    }
}
