//! After a won battle (§7.3, §7.4, §8.1): experience, level ups, learned moves and
//! evolutions, played back before play returns to the overworld.
//!
//! 1. **Default exp.** [`battle_rewards`] adds up the exp of every opponent creature
//!    ([`defeat_exp`]: `exp_yield · level / 7`, ×1.5 against trainers). The battle starts its
//!    result with it on a player win, and `on_battle_end` hooks may change it (§7.4).
//! 2. **Distribution.** [`PostBattle::new`] splits the final `result.exp` evenly between the
//!    *earners*: the player's creatures that were sent out during the battle and have not
//!    fainted ([`split_exp`]).
//! 3. **Level ups.** Each earner gains its share along its species' growth curve
//!    ([`PartyMember::gain_exp`]); its stats follow its level. Every level reached shows a
//!    text, then the moves of the learnset at that level.
//! 4. **Moves.** A move goes into a free slot; with four moves known the player picks the
//!    move to forget, or "Don't learn" (Cancel too), with the keyboard.
//! 5. **Evolution.** A creature that levelled up in this battle and reached a level
//!    evolution's level evolves after all exp texts: "What? … is evolving!", the
//!    [`EvolutionScene`], "Congratulations! …", then the moves its new species learns at its
//!    level.
//!
//! The flow works on its own copy of the party; the host keeps the result when the flow is
//! done ([`PostBattle::into_party`]).

use std::collections::VecDeque;

use pkrs_battle::{
    BattleOutcome, BattleResult, BattleRewards, BattleSetup, PartyIndex, defeat_exp, split_exp,
};
use pkrs_data::BattlePack;
use pkrs_platform::{Action, BuiltinAction, Input};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_ui::{
    ChoiceOption, ChoicePollOutcome, ChoiceSession, DialogueAdvance, DialogueSession,
    colored_line_char_count, draw_colored_lines, draw_more_arrow,
};

use super::content::BattleKind;
use super::screen::{BATTLE_CHARS_PER_TICK, TEXT_PANEL_HEIGHT};
use crate::evolution::EvolutionScene;
use crate::locale::BattleText;
use crate::party::{Learn, MAX_MOVES, PlayerParty};
use crate::ui_theme::UiTheme;

/// Default rewards of beating `setup`'s opponent side: the [`defeat_exp`] of every opponent
/// creature (trainer bonus for [`BattleKind::Trainer`]); no prize money yet.
#[must_use]
pub fn battle_rewards(kind: &BattleKind, setup: &BattleSetup, pack: &BattlePack) -> BattleRewards {
    let trainer = matches!(kind, BattleKind::Trainer { .. });
    let exp = setup
        .opponent
        .party
        .iter()
        .map(|c| {
            let base = pack.species.get(&c.species_id).map_or(0, |s| s.exp_yield);
            defeat_exp(base, c.level, trainer)
        })
        .fold(0_u32, u32::saturating_add);
    BattleRewards { exp, money: 0 }
}

/// Which part of the flow is on screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostBattlePhase {
    /// Exp, level-up and move texts over the battle scene.
    Rewards,
    /// The evolution scene (from the first evolution on).
    Evolution,
}

/// One queued step.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Step {
    Text(String),
    Learn { member: usize, move_id: String },
    Prompt { member: usize, move_id: String },
    Evolve { member: usize, into: String },
    Animate { member: usize, into: String },
}

/// What is on screen.
enum Playing {
    Text {
        text: String,
        session: DialogueSession,
    },
    Forget {
        member: usize,
        move_id: String,
        prompt: String,
        session: ChoiceSession,
    },
    Evolving {
        member: usize,
        into: String,
    },
}

/// The post-battle flow: texts, the move-forget prompt and evolutions.
pub struct PostBattle {
    party: PlayerParty,
    pack: BattlePack,
    steps: VecDeque<Step>,
    playing: Option<Playing>,
    phase: PostBattlePhase,
    scene: Option<EvolutionScene>,
    theme: UiTheme,
    view: (u32, u32),
    texts_shown: u64,
    /// Localized texts and names (§5.5).
    text: BattleText,
}

impl PostBattle {
    /// The flow for `result`: `earners` (party indices sent out and not fainted) share
    /// `result.exp`. `None` when there is nothing to show (not a win, no exp, no earner).
    /// Texts are in English; see [`with_text`](Self::with_text).
    #[must_use]
    pub fn new(
        result: &BattleResult,
        earners: &[PartyIndex],
        party: &PlayerParty,
        pack: &BattlePack,
        theme: UiTheme,
        view: (u32, u32),
    ) -> Option<Self> {
        Self::with_text(
            result,
            earners,
            party,
            pack,
            theme,
            view,
            BattleText::default(),
        )
    }

    /// Like [`new`](Self::new), with texts and names from `text`.
    #[must_use]
    pub fn with_text(
        result: &BattleResult,
        earners: &[PartyIndex],
        party: &PlayerParty,
        pack: &BattlePack,
        theme: UiTheme,
        view: (u32, u32),
        text: BattleText,
    ) -> Option<Self> {
        if result.outcome != BattleOutcome::PlayerWin || result.exp == 0 {
            return None;
        }
        let earners: Vec<usize> = earners
            .iter()
            .map(|&i| usize::from(i))
            .filter(|&i| party.member(i).is_some())
            .collect();
        if earners.is_empty() {
            return None;
        }
        let mut party = party.clone();
        let mut steps = VecDeque::new();
        let mut evolutions = Vec::new();
        for (member, share) in earners
            .iter()
            .copied()
            .zip(split_exp(result.exp, earners.len()))
        {
            let Some(m) = party.member_mut(member) else {
                continue;
            };
            let name = text.species(&m.species);
            steps.push_back(Step::Text(text.say(
                "battle.exp_gained",
                &[("name", &name), ("exp", &share.to_string())],
            )));
            let Some(species) = pack.species.get(&m.species) else {
                tracing::warn!(target: "battle", species = %m.species, "unknown species gains no levels");
                continue;
            };
            let levels = m.gain_exp(share, species);
            for level in &levels {
                steps.push_back(Step::Text(text.say(
                    "battle.level_up",
                    &[("name", &name), ("level", &level.to_string())],
                )));
                for move_id in species.moves_learned_at(*level) {
                    steps.push_back(Step::Learn {
                        member,
                        move_id: move_id.to_owned(),
                    });
                }
            }
            if !levels.is_empty()
                && let Some(into) = species.evolution_at(m.level)
            {
                if pack.species.contains_key(into) {
                    evolutions.push(Step::Evolve {
                        member,
                        into: into.to_owned(),
                    });
                } else {
                    tracing::warn!(target: "battle", species = %m.species, into, "evolution into an unknown species skipped");
                }
            }
        }
        steps.extend(evolutions);
        Some(Self {
            party,
            pack: pack.clone(),
            steps,
            playing: None,
            phase: PostBattlePhase::Rewards,
            scene: None,
            theme,
            view,
            texts_shown: 0,
            text,
        })
    }

    /// Which part is on screen.
    #[must_use]
    pub fn phase(&self) -> PostBattlePhase {
        self.phase
    }

    /// The party as the flow has changed it so far.
    #[must_use]
    pub fn party(&self) -> &PlayerParty {
        &self.party
    }

    /// The party with every change of the flow.
    #[must_use]
    pub fn into_party(self) -> PlayerParty {
        self.party
    }

    /// `true` once every step was played.
    #[must_use]
    pub fn is_done(&self) -> bool {
        self.playing.is_none() && self.steps.is_empty()
    }

    /// The text on screen: a message, or the prompt of the move-forget menu.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match &self.playing {
            Some(Playing::Text { text, .. }) => Some(text),
            Some(Playing::Forget { prompt, .. }) => Some(prompt),
            _ => None,
        }
    }

    /// Rows of the move-forget menu (empty when it is not open).
    #[must_use]
    pub fn menu_labels(&self) -> Vec<String> {
        match &self.playing {
            Some(Playing::Forget { session, .. }) => {
                session.options().iter().map(|o| o.label.clone()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Highlighted row of the move-forget menu (0-based).
    #[must_use]
    pub fn menu_cursor(&self) -> Option<usize> {
        match &self.playing {
            Some(Playing::Forget { session, .. }) => Some(session.cursor()),
            _ => None,
        }
    }

    /// The evolution scene, from the first evolution on.
    #[must_use]
    pub fn scene(&self) -> Option<&EvolutionScene> {
        self.scene.as_ref()
    }

    /// Messages shown so far.
    #[must_use]
    pub fn texts_shown(&self) -> u64 {
        self.texts_shown
    }

    /// One tick; returns `true` once the flow is done.
    pub fn update(&mut self, input: &mut Input) -> bool {
        if self.playing.is_none() {
            self.next_step();
        }
        let confirm = input.triggered(Action::Builtin(BuiltinAction::Confirm));
        match self.playing.as_mut() {
            None => {}
            Some(Playing::Text { session, .. }) => {
                if confirm {
                    input.consume(Action::Builtin(BuiltinAction::Confirm));
                }
                if session.tick(confirm) == DialogueAdvance::Done {
                    self.playing = None;
                }
            }
            Some(Playing::Forget { session, .. }) => {
                let picked = session.poll(input);
                if let Some(outcome) = picked {
                    self.forget(input, outcome);
                }
            }
            Some(Playing::Evolving { .. }) => {
                let done = self.scene.as_mut().is_none_or(EvolutionScene::tick);
                if done {
                    self.evolved();
                }
            }
        }
        self.is_done()
    }

    /// Starts the next step (texts and prompts wait; learning into a free slot does not).
    fn next_step(&mut self) {
        while self.playing.is_none() {
            let Some(step) = self.steps.pop_front() else {
                return;
            };
            match step {
                Step::Text(text) => self.say(text),
                Step::Learn { member, move_id } => self.learn(member, move_id),
                Step::Prompt { member, move_id } => self.prompt(member, move_id),
                Step::Evolve { member, into } => {
                    let Some(m) = self.party.member(member) else {
                        continue;
                    };
                    let name = self.text.species(&m.species);
                    self.phase = PostBattlePhase::Evolution;
                    self.scene = Some(EvolutionScene::new(name.clone(), self.text.species(&into)));
                    self.steps.push_front(Step::Animate { member, into });
                    let text = self.text.say("battle.evolving", &[("name", &name)]);
                    self.say(text);
                }
                Step::Animate { member, into } => {
                    self.playing = Some(Playing::Evolving { member, into });
                }
            }
        }
    }

    fn say(&mut self, text: String) {
        self.texts_shown = self.texts_shown.saturating_add(1);
        let panel = text_panel(self.view);
        let session = DialogueSession::new(
            &text,
            &self.theme.font,
            panel.width - 16.0,
            panel.height - 12.0,
            1,
            BATTLE_CHARS_PER_TICK,
            &mut |_| String::new(),
        );
        self.playing = Some(Playing::Text { text, session });
    }

    fn learn(&mut self, member: usize, move_id: String) {
        let Some(m) = self.party.member_mut(member) else {
            return;
        };
        let (name, move_name) = (self.text.species(&m.species), self.text.move_name(&move_id));
        match m.learn(&move_id) {
            Learn::AlreadyKnown => {}
            Learn::Learned => {
                let text = self
                    .text
                    .say("battle.learned", &[("name", &name), ("move", &move_name)]);
                self.say(text);
            }
            Learn::Full => {
                self.steps.push_front(Step::Prompt { member, move_id });
                let text = self.text.say(
                    "battle.moves_full",
                    &[
                        ("name", &name),
                        ("move", &move_name),
                        ("max", &MAX_MOVES.to_string()),
                    ],
                );
                self.say(text);
            }
        }
    }

    /// Opens the move-forget menu: the known moves, then "Don't learn …".
    fn prompt(&mut self, member: usize, move_id: String) {
        let Some(m) = self.party.member(member) else {
            return;
        };
        let (name, move_name) = (self.text.species(&m.species), self.text.move_name(&move_id));
        let mut options: Vec<ChoiceOption> = m
            .moves
            .iter()
            .map(|id| ChoiceOption {
                label: self.text.move_name(id),
            })
            .collect();
        options.push(ChoiceOption {
            label: self.text.say("battle.dont_learn", &[("move", &move_name)]),
        });
        let prompt = self.text.say("battle.forget_prompt", &[("name", &name)]);
        match ChoiceSession::new(options) {
            Ok(session) => {
                self.texts_shown = self.texts_shown.saturating_add(1);
                self.playing = Some(Playing::Forget {
                    member,
                    move_id,
                    prompt,
                    session,
                });
            }
            Err(err) => tracing::error!(target: "battle", %err, "move prompt"),
        }
    }

    /// The player answered the move-forget menu.
    fn forget(&mut self, input: &mut Input, outcome: ChoicePollOutcome) {
        let Some(Playing::Forget {
            member, move_id, ..
        }) = self.playing.take()
        else {
            return;
        };
        let slot = match outcome {
            ChoicePollOutcome::Picked(i) => {
                input.consume(Action::Builtin(BuiltinAction::Confirm));
                Some(i)
            }
            ChoicePollOutcome::Cancelled => {
                input.consume(Action::Builtin(BuiltinAction::Cancel));
                None
            }
        };
        let Some(m) = self.party.member_mut(member) else {
            return;
        };
        let (name, move_name) = (self.text.species(&m.species), self.text.move_name(&move_id));
        let forgotten = slot.and_then(|i| m.replace_move(i, &move_id));
        match forgotten {
            Some(old) => {
                let old = self.text.move_name(&old);
                self.steps.push_front(Step::Text(self.text.say(
                    "battle.forgot_and_learned",
                    &[("name", &name), ("old", &old), ("move", &move_name)],
                )));
                let text = self.text.say("battle.poof", &[]);
                self.say(text);
            }
            None => {
                let text = self.text.say(
                    "battle.did_not_learn",
                    &[("name", &name), ("move", &move_name)],
                );
                self.say(text);
            }
        }
    }

    /// The evolution animation ended: the member becomes its new species.
    fn evolved(&mut self) {
        let Some(Playing::Evolving { member, into }) = self.playing.take() else {
            return;
        };
        let (Some(m), Some(species)) =
            (self.party.member_mut(member), self.pack.species.get(&into))
        else {
            return;
        };
        let old = self.text.species(&m.species);
        m.evolve(species);
        let level = m.level;
        let new_moves: Vec<Step> = species
            .moves_learned_at(level)
            .map(|id| Step::Learn {
                member,
                move_id: id.to_owned(),
            })
            .collect();
        for step in new_moves.into_iter().rev() {
            self.steps.push_front(step);
        }
        let text = self.text.say(
            "battle.evolved",
            &[("name", &old), ("into", &self.text.species(&into))],
        );
        self.say(text);
    }

    // --- rendering ---------------------------------------------------------------------

    /// Draws the flow: the evolution scene once it started (otherwise the caller draws the
    /// battle scene first), the text panel and the move-forget menu.
    pub fn render(&self, renderer: &mut Renderer) {
        if let Some(scene) = &self.scene {
            scene.render(renderer, self.view);
        }
        let panel = text_panel(self.view);
        self.theme.skin.draw(renderer, panel, PANEL_TINT);
        let (tx, ty) = (panel.x + 8.0, panel.y + 6.0);
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        match &self.playing {
            Some(Playing::Text { session, .. }) => {
                let vis = session.visible_glyphs();
                draw_colored_lines(
                    renderer,
                    &self.theme.font,
                    self.theme.font_tex,
                    session.lines(),
                    vis,
                    tx,
                    ty,
                    1,
                    Layer::Ui,
                );
                if session.waiting_for_confirm && vis >= colored_line_char_count(session.lines()) {
                    draw_more_arrow(
                        renderer,
                        self.theme.font_tex,
                        panel.x + panel.width - 16.0,
                        panel.y + panel.height - 14.0,
                        Color::WHITE,
                    );
                }
            }
            Some(Playing::Forget {
                prompt, session, ..
            }) => {
                renderer.draw_text(&self.theme.font, self.theme.font_tex, prompt, tx, ty, style);
                let lh = self.theme.font.line_height.max(1) as f32;
                let rows = session.options().len() as f32;
                let (vw, vh) = (self.view.0 as f32, self.view.1 as f32);
                let height = rows * lh + 12.0;
                let rect = Rect::new(
                    vw - 128.0,
                    (vh - TEXT_PANEL_HEIGHT - height).max(0.0),
                    128.0,
                    height,
                );
                self.theme.skin.draw(renderer, rect, PANEL_TINT);
                for (i, option) in session.options().iter().enumerate() {
                    let prefix = if session.cursor() == i { "> " } else { "  " };
                    renderer.draw_text(
                        &self.theme.font,
                        self.theme.font_tex,
                        &format!("{prefix}{}", option.label),
                        rect.x + 6.0,
                        rect.y + 6.0 + lh * i as f32,
                        style,
                    );
                }
            }
            _ => {}
        }
    }
}

fn text_panel(view: (u32, u32)) -> Rect {
    let (vw, vh) = (view.0 as f32, view.1 as f32);
    Rect::new(0.0, vh - TEXT_PANEL_HEIGHT, vw, TEXT_PANEL_HEIGHT)
}

const PANEL_TINT: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 220,
};

#[cfg(test)]
mod tests;
