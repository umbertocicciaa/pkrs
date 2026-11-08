//! The battle screen: menus, event playback queue and scene rendering (§7.2, §4.1).
//!
//! The logic never waits on rendering: when the player confirms an action, the screen asks
//! the AI for the opponent's input and steps the [`Battle`] until it needs the player again
//! (next turn, a forced switch) or ends, including the opponent's forced replacements. The
//! resulting events are mapped to [`Cue`]s by the [`Presenter`] and queued; the screen then
//! plays them one per tick onwards (text boxes wait for Confirm, HP bars and sprites animate)
//! and opens the next menu only when the queue is empty.

use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use pkrs_battle::{
    ActionMenu, Battle, BattleAction, BattleError, BattleHooks, BattleInput, BattleOutcome,
    BattlePhase, BattleResult, PartyIndex, RngPolicy, SideId, SideInput, SlotIndex, SlotRef,
    TrainerAi, ai_side_input,
};
use pkrs_data::BattlePack;
use pkrs_platform::{Action, BuiltinAction, Input};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_script::LuaScriptHost;
use pkrs_ui::{
    ChoiceOption, ChoicePollOutcome, ChoiceSession, DialogueAdvance, DialogueSession,
    colored_line_char_count, draw_colored_lines, draw_more_arrow,
};

use super::content::{BattleKind, PreparedBattle};
use super::menu::{
    ActionKind, MenuEntry, action_entries, bag_entries, fight_entries, party_entries,
};
use super::present::{Cue, Presenter, Roster};
use crate::ui_theme::UiTheme;

/// Glyphs revealed per tick in battle text.
pub const BATTLE_CHARS_PER_TICK: u32 = 2;
/// Ticks of a faint animation.
pub const FAINT_TICKS: u32 = 20;
/// Ticks of a switch-in or switch-out animation.
pub const SWITCH_TICKS: u32 = 12;
/// HP bars move at least this fraction (1/n) of max HP per tick.
pub const HP_BAR_STEPS: u32 = 32;
/// Height of the bottom text panel at internal resolution.
pub const TEXT_PANEL_HEIGHT: f32 = 48.0;
/// Upper bound of automatic logic steps per player input (opponent replacements).
const MAX_AUTO_STEPS: usize = 16;

/// Which menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuKind {
    /// Fight / bag / party / run (+ hook entries).
    Action,
    /// Moves with PP.
    Fight,
    /// Items.
    Bag,
    /// Party; `forced` after the active creature fainted (cannot be cancelled).
    Party {
        /// Mandatory replacement.
        forced: bool,
    },
}

impl MenuKind {
    /// Name used by the test harness (`action`, `fight`, `bag`, `party`).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Action => "action",
            Self::Fight => "fight",
            Self::Bag => "bag",
            Self::Party { .. } => "party",
        }
    }
}

enum MenuValues {
    Action(Vec<ActionKind>),
    Fight(Vec<u8>),
    Bag(Vec<String>),
    Party(Vec<PartyIndex>),
}

struct OpenMenu {
    kind: MenuKind,
    session: ChoiceSession,
    enabled: Vec<bool>,
    values: MenuValues,
}

impl OpenMenu {
    fn new<T>(
        kind: MenuKind,
        entries: Vec<MenuEntry<T>>,
        wrap: impl FnOnce(Vec<T>) -> MenuValues,
    ) -> Option<Self> {
        let options = entries
            .iter()
            .map(|e| ChoiceOption {
                label: e.label.clone(),
            })
            .collect();
        let session = ChoiceSession::new(options).ok()?;
        let enabled = entries.iter().map(|e| e.enabled).collect();
        let values = wrap(entries.into_iter().map(|e| e.value).collect());
        Some(Self {
            kind,
            session,
            enabled,
            values,
        })
    }
}

/// The cue being played.
enum Playing {
    Text {
        text: String,
        session: DialogueSession,
    },
    Hp {
        side: SideId,
        party: PartyIndex,
        to: u32,
        step: u32,
    },
    Anim {
        cue: Cue,
        total: u32,
        left: u32,
    },
}

/// Visibility of the sprite in each side's first slot.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SpriteFx {
    visible: bool,
}

/// A running battle on screen.
pub struct BattleScreen {
    battle: Battle,
    pack: Rc<BattlePack>,
    ai: TrainerAi,
    kind: BattleKind,
    presenter: Presenter,
    display: Roster,
    queue: VecDeque<Cue>,
    playing: Option<Playing>,
    menu: Option<OpenMenu>,
    reopen: Option<MenuKind>,
    action_menu: Option<(u32, ActionMenu)>,
    bag: BTreeMap<String, u32>,
    sprites: [SpriteFx; 2],
    /// Active FX asset id per side's lead slot (`ctx.play_fx`, §9.5).
    slot_fx: [Option<String>; 2],
    theme: UiTheme,
    view: (u32, u32),
    texts_shown: u64,
    finished: bool,
}

impl BattleScreen {
    /// Starts `prepared` (RNG from `policy`) on a `view`-sized screen. `bag` fills the bag
    /// menu. The opening events are queued for playback right away.
    pub fn new(
        prepared: PreparedBattle,
        policy: RngPolicy,
        theme: UiTheme,
        view: (u32, u32),
        bag: BTreeMap<String, u32>,
        hooks: &mut dyn BattleHooks,
    ) -> Result<Self, BattleError> {
        let PreparedBattle {
            kind,
            setup,
            resources,
            ai,
        } = prepared;
        let presenter = Presenter::new(kind.clone(), &setup);
        let display = presenter.roster().clone();
        let pack = Rc::new(resources.pack.clone());
        let battle = Battle::new(
            setup,
            pkrs_battle::BattleConfig::default(),
            resources,
            policy,
            hooks,
        )?;
        let mut screen = Self {
            battle,
            pack,
            ai,
            kind,
            presenter,
            display,
            queue: VecDeque::new(),
            playing: None,
            menu: None,
            reopen: None,
            action_menu: None,
            bag,
            sprites: [SpriteFx { visible: true }; 2],
            slot_fx: [None, None],
            theme,
            view,
            texts_shown: 0,
            finished: false,
        };
        screen.run_logic(BattleInput::default(), hooks);
        Ok(screen)
    }

    /// [`new`](Self::new) with the battle hooks of `host` (`battle.on(...)` handlers, §7.4).
    pub fn with_scripts(
        prepared: PreparedBattle,
        policy: RngPolicy,
        theme: UiTheme,
        view: (u32, u32),
        bag: BTreeMap<String, u32>,
        host: &LuaScriptHost,
    ) -> Result<Self, BattleError> {
        let pack = prepared.resources.pack.clone();
        let mut hooks = host.battle_hooks().with_moves(&pack);
        Self::new(prepared, policy, theme, view, bag, &mut hooks)
    }

    /// One tick with the battle hooks of `host` (see [`update`](Self::update)).
    pub fn update_with_scripts(
        &mut self,
        input: &mut Input,
        host: &LuaScriptHost,
    ) -> Option<BattleResult> {
        let pack = Rc::clone(&self.pack);
        let mut hooks = host.battle_hooks().with_moves(&pack);
        self.update(input, &mut hooks)
    }

    /// One tick: plays the next cue or drives the open menu. Returns the result once, on
    /// the tick the last cue of a finished battle was played.
    pub fn update(
        &mut self,
        input: &mut Input,
        hooks: &mut dyn BattleHooks,
    ) -> Option<BattleResult> {
        if self.finished {
            return None;
        }
        if self.menu.is_some() {
            self.tick_menu(input, hooks);
            return None;
        }
        let confirm = take(input, BuiltinAction::Confirm);
        self.tick_playback(confirm);
        if self.playing.is_none() && self.queue.is_empty() {
            return self.after_playback(hooks);
        }
        None
    }

    /// `true` once the result was handed out.
    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Wild or trainer battle.
    #[must_use]
    pub fn kind(&self) -> &BattleKind {
        &self.kind
    }

    /// The battle logic (read-only).
    #[must_use]
    pub fn battle(&self) -> &Battle {
        &self.battle
    }

    /// What the screen currently shows: HP as drawn on the bars, active creatures.
    #[must_use]
    pub fn displayed(&self) -> &Roster {
        &self.display
    }

    /// The battle message on screen, if one is shown.
    #[must_use]
    pub fn text(&self) -> Option<&str> {
        match &self.playing {
            Some(Playing::Text { text, .. }) => Some(text),
            _ => None,
        }
    }

    /// Battle messages shown so far.
    #[must_use]
    pub fn texts_shown(&self) -> u64 {
        self.texts_shown
    }

    /// The open menu.
    #[must_use]
    pub fn menu(&self) -> Option<MenuKind> {
        self.menu.as_ref().map(|m| m.kind)
    }

    /// Rows of the open menu (empty when none is open).
    #[must_use]
    pub fn menu_labels(&self) -> Vec<String> {
        self.menu.as_ref().map_or_else(Vec::new, |m| {
            m.session
                .options()
                .iter()
                .map(|o| o.label.clone())
                .collect()
        })
    }

    /// Highlighted row of the open menu (0-based).
    #[must_use]
    pub fn menu_cursor(&self) -> Option<usize> {
        self.menu.as_ref().map(|m| m.session.cursor())
    }

    /// Cues still waiting to be played (not counting the one playing).
    #[must_use]
    pub fn queued(&self) -> &VecDeque<Cue> {
        &self.queue
    }

    // --- logic -------------------------------------------------------------------------

    /// Steps the battle with `input`, then keeps stepping through the opponent's forced
    /// replacements, queueing the cues of every event.
    fn run_logic(&mut self, input: BattleInput, hooks: &mut dyn BattleHooks) {
        let mut input = input;
        for _ in 0..MAX_AUTO_STEPS {
            match self.battle.step(input, hooks) {
                Ok(events) => {
                    let cues = self.presenter.present_all(&events);
                    self.queue.extend(cues);
                }
                Err(err) => {
                    tracing::warn!(target: "battle", %err, "battle input rejected");
                    self.queue
                        .push_back(Cue::Text("That can't be done now!".into()));
                    return;
                }
            }
            match self.battle.phase() {
                BattlePhase::Replace {
                    side: SideId::Opponent,
                    ..
                } => {
                    input = BattleInput {
                        player: None,
                        opponent: ai_side_input(&mut self.ai, &self.battle, SideId::Opponent),
                    };
                }
                _ => return,
            }
        }
        tracing::error!(target: "battle", "battle did not settle after {MAX_AUTO_STEPS} steps");
    }

    fn submit(&mut self, action: BattleAction, hooks: &mut dyn BattleHooks) {
        let input = match self.battle.phase() {
            BattlePhase::Replace {
                side: SideId::Player,
                ..
            } => BattleInput {
                player: Some(SideInput {
                    slots: vec![action],
                }),
                opponent: None,
            },
            _ => BattleInput {
                player: Some(SideInput {
                    slots: vec![action],
                }),
                opponent: ai_side_input(&mut self.ai, &self.battle, SideId::Opponent),
            },
        };
        self.menu = None;
        self.run_logic(input, hooks);
    }

    /// The queue is empty: end the battle, or open the menu the battle waits for.
    fn after_playback(&mut self, hooks: &mut dyn BattleHooks) -> Option<BattleResult> {
        match self.battle.phase() {
            BattlePhase::End => {
                self.finished = true;
                let result = self.battle.result().copied().unwrap_or_else(|| {
                    tracing::error!(target: "battle", "battle ended without a result");
                    BattleResult {
                        outcome: BattleOutcome::Fled {
                            side: SideId::Player,
                        },
                        turns: self.battle.turn(),
                        exp: 0,
                        money: 0,
                    }
                });
                Some(result)
            }
            BattlePhase::Replace {
                side: SideId::Player,
                ..
            } => {
                self.reopen = None;
                self.open_menu(MenuKind::Party { forced: true }, hooks);
                None
            }
            BattlePhase::SelectActions => {
                let kind = self.reopen.take().unwrap_or(MenuKind::Action);
                self.open_menu(kind, hooks);
                None
            }
            phase => {
                // `run_logic` always stops at a phase that needs the player or at the end.
                tracing::error!(target: "battle", ?phase, "battle stuck; ending it");
                self.finished = true;
                None
            }
        }
    }

    fn open_menu(&mut self, kind: MenuKind, hooks: &mut dyn BattleHooks) {
        let view = self.battle.view();
        let me = SlotRef::new(SideId::Player, 0);
        let menu = match kind {
            MenuKind::Action => {
                let turn = self.battle.turn();
                let menu = match &self.action_menu {
                    Some((t, menu)) if *t == turn => menu.clone(),
                    _ => {
                        let menu = self.battle.action_menu(me, hooks).unwrap_or_else(|err| {
                            tracing::warn!(target: "battle", %err, "on_action_menu failed");
                            ActionMenu::builtin(me)
                        });
                        self.action_menu = Some((turn, menu.clone()));
                        menu
                    }
                };
                OpenMenu::new(kind, action_entries(&menu), MenuValues::Action)
            }
            MenuKind::Fight => view
                .creature(me)
                .and_then(|c| OpenMenu::new(kind, fight_entries(c), MenuValues::Fight)),
            MenuKind::Bag => OpenMenu::new(kind, bag_entries(&self.bag), MenuValues::Bag),
            MenuKind::Party { .. } => OpenMenu::new(
                kind,
                party_entries(view.side(SideId::Player)),
                MenuValues::Party,
            ),
        };
        match menu {
            Some(menu) => self.menu = Some(menu),
            None => {
                let text = match kind {
                    MenuKind::Bag => "The bag is empty.",
                    _ => "There's nothing to choose.",
                };
                self.say_then(text, MenuKind::Action);
            }
        }
    }

    /// Queues `text`, then reopens `menu` once it was read.
    fn say_then(&mut self, text: impl Into<String>, menu: MenuKind) {
        self.menu = None;
        self.queue.push_back(Cue::Text(text.into()));
        self.reopen = Some(menu);
    }

    fn tick_menu(&mut self, input: &mut Input, hooks: &mut dyn BattleHooks) {
        let Some(menu) = self.menu.as_mut() else {
            return;
        };
        let Some(outcome) = menu.session.poll(input) else {
            return;
        };
        match outcome {
            ChoicePollOutcome::Picked(i) => {
                input.consume(Action::Builtin(BuiltinAction::Confirm));
                let enabled = menu.enabled.get(i).copied().unwrap_or(false);
                let kind = menu.kind;
                let pick = match &menu.values {
                    MenuValues::Action(v) => v.get(i).cloned().map(Pick::Action),
                    MenuValues::Fight(v) => v.get(i).copied().map(Pick::Move),
                    MenuValues::Bag(v) => v.get(i).cloned().map(Pick::Item),
                    MenuValues::Party(v) => v.get(i).copied().map(Pick::Member),
                };
                if let Some(pick) = pick {
                    self.pick(kind, pick, enabled, hooks);
                }
            }
            ChoicePollOutcome::Cancelled => {
                input.consume(Action::Builtin(BuiltinAction::Cancel));
                match menu.kind {
                    MenuKind::Action | MenuKind::Party { forced: true } => {}
                    _ => self.open_menu(MenuKind::Action, hooks),
                }
            }
        }
    }

    fn pick(&mut self, kind: MenuKind, pick: Pick, enabled: bool, hooks: &mut dyn BattleHooks) {
        match pick {
            Pick::Action(ActionKind::Fight) => self.open_menu(MenuKind::Fight, hooks),
            Pick::Action(ActionKind::Bag) => self.open_menu(MenuKind::Bag, hooks),
            Pick::Action(ActionKind::Party) => {
                self.open_menu(MenuKind::Party { forced: false }, hooks);
            }
            Pick::Action(ActionKind::Run) => match self.kind {
                BattleKind::Wild => self.submit(BattleAction::Run, hooks),
                BattleKind::Trainer { .. } => self.say_then(
                    "There's no running from a trainer battle!",
                    MenuKind::Action,
                ),
            },
            Pick::Action(ActionKind::Custom(id)) => self.submit(
                BattleAction::Custom {
                    id,
                    payload: Vec::new(),
                },
                hooks,
            ),
            Pick::Move(_) if !enabled => {
                self.say_then("There's no PP left for this move!", MenuKind::Fight);
            }
            Pick::Move(move_slot) => self.submit(
                BattleAction::Fight {
                    move_slot,
                    target_side: SideId::Opponent,
                    target_slot: 0,
                },
                hooks,
            ),
            Pick::Item(item_id) => self.submit(
                BattleAction::Item {
                    item_id,
                    target_side: SideId::Player,
                    target_slot: 0,
                },
                hooks,
            ),
            Pick::Member(party) if !enabled => {
                let member = self
                    .display
                    .side(SideId::Player)
                    .members
                    .get(usize::from(party));
                let text = match member {
                    Some(m) if m.hp == 0 => format!("{} has no energy left to battle!", m.name),
                    Some(m) => format!("{} is already in battle!", m.name),
                    None => "That creature can't battle.".to_owned(),
                };
                self.say_then(text, kind);
            }
            Pick::Member(to_party) => self.submit(BattleAction::Switch { to_party }, hooks),
        }
    }

    // --- playback ----------------------------------------------------------------------

    fn tick_playback(&mut self, confirm: bool) {
        while self.playing.is_none() {
            let Some(cue) = self.queue.pop_front() else {
                return;
            };
            if let Cue::Fx { side, slot, id } = cue {
                self.set_slot_fx(side, slot, id);
                continue;
            }
            self.playing = Some(self.start_cue(cue));
            break;
        }
        let done = match self.playing.as_mut() {
            None => false,
            Some(Playing::Text { session, .. }) => session.tick(confirm) == DialogueAdvance::Done,
            Some(Playing::Hp {
                side,
                party,
                to,
                step,
            }) => {
                let (side, party, to, step) = (*side, *party, *to, *step);
                let hp = self
                    .display
                    .side(side)
                    .members
                    .get(usize::from(party))
                    .map_or(to, |m| m.hp);
                let next = if hp > to {
                    hp.saturating_sub(step).max(to)
                } else {
                    hp.saturating_add(step).min(to)
                };
                self.display.set_hp(side, party, next);
                next == to
            }
            Some(Playing::Anim { left, .. }) => {
                *left = left.saturating_sub(1);
                *left == 0
            }
        };
        if done {
            if let Some(Playing::Anim { cue, .. }) = self.playing.take() {
                self.finish_anim(&cue);
            }
        }
    }

    fn start_cue(&mut self, cue: Cue) -> Playing {
        match cue {
            Cue::Text(text) => {
                self.texts_shown = self.texts_shown.saturating_add(1);
                let panel = self.text_panel();
                let session = DialogueSession::new(
                    &text,
                    &self.theme.font,
                    panel.width - 16.0,
                    panel.height - 12.0,
                    1,
                    BATTLE_CHARS_PER_TICK,
                    &mut |_| String::new(),
                );
                Playing::Text { text, session }
            }
            Cue::Hp {
                side,
                party,
                from,
                to,
            } => {
                self.display.set_hp(side, party, from);
                let max = self
                    .display
                    .side(side)
                    .members
                    .get(usize::from(party))
                    .map_or(1, |m| m.max_hp);
                Playing::Hp {
                    side,
                    party,
                    to,
                    step: (max / HP_BAR_STEPS).max(1),
                }
            }
            Cue::SwitchIn { side, slot, .. } => {
                self.display.apply(&cue);
                if slot == 0 {
                    self.sprites[side_index(side)].visible = true;
                }
                Playing::Anim {
                    cue,
                    total: SWITCH_TICKS,
                    left: SWITCH_TICKS,
                }
            }
            Cue::SwitchOut { .. } => Playing::Anim {
                cue,
                total: SWITCH_TICKS,
                left: SWITCH_TICKS,
            },
            Cue::Faint { .. } => Playing::Anim {
                cue,
                total: FAINT_TICKS,
                left: FAINT_TICKS,
            },
            Cue::Fx { .. } => {
                tracing::error!(target: "battle", "Fx cue must not reach start_cue");
                Playing::Anim {
                    cue,
                    total: 0,
                    left: 0,
                }
            }
        }
    }

    fn set_slot_fx(&mut self, side: SideId, slot: SlotIndex, id: Option<String>) {
        if slot != 0 {
            tracing::warn!(target: "battle", ?side, slot, "battle FX on non-lead slots is ignored");
            return;
        }
        let idx = side_index(side);
        if let Some(id) = id {
            tracing::debug!(target: "battle", fx = %id, ?side, "play battle FX");
            self.slot_fx[idx] = Some(id);
        } else {
            self.slot_fx[idx] = None;
        }
    }

    fn finish_anim(&mut self, cue: &Cue) {
        match *cue {
            Cue::Faint { side, slot } | Cue::SwitchOut { side, slot } if slot == 0 => {
                self.sprites[side_index(side)].visible = false;
            }
            _ => {}
        }
    }

    // --- rendering ---------------------------------------------------------------------

    fn text_panel(&self) -> Rect {
        let (vw, vh) = (self.view.0 as f32, self.view.1 as f32);
        Rect::new(0.0, vh - TEXT_PANEL_HEIGHT, vw, TEXT_PANEL_HEIGHT)
    }

    /// How far the sprite of `side` is shown (`0.0` hidden … `1.0` full) this frame.
    fn sprite_scale(&self, side: SideId) -> f32 {
        if let Some(Playing::Anim { cue, total, left }) = &self.playing {
            let t = 1.0 - *left as f32 / (*total).max(1) as f32;
            match *cue {
                Cue::SwitchIn {
                    side: s, slot: 0, ..
                } if s == side => return t,
                Cue::SwitchOut { side: s, slot: 0 } | Cue::Faint { side: s, slot: 0 }
                    if s == side =>
                {
                    return 1.0 - t;
                }
                _ => {}
            }
        }
        if self.sprites[side_index(side)].visible {
            1.0
        } else {
            0.0
        }
    }

    /// Draws the scene at internal resolution (between `begin_frame` and `end_frame`).
    pub fn render(&self, renderer: &mut Renderer) {
        let (vw, vh) = (self.view.0 as f32, self.view.1 as f32);
        let field_h = vh - TEXT_PANEL_HEIGHT;
        renderer.draw_filled_rect(Rect::new(0.0, 0.0, vw, field_h), SKY, Layer::Ground);
        renderer.draw_filled_rect(
            Rect::new(0.0, field_h * 0.55, vw, field_h * 0.45),
            GROUND,
            Layer::Ground,
        );
        // Platforms and creatures (placeholder art: coloured blocks).
        let foe_base = Rect::new(vw - 96.0, field_h * 0.42, 80.0, 8.0);
        let my_base = Rect::new(12.0, field_h - 10.0, 96.0, 8.0);
        renderer.draw_filled_rect(foe_base, PLATFORM, Layer::GroundDecor);
        renderer.draw_filled_rect(my_base, PLATFORM, Layer::GroundDecor);
        self.draw_creature(renderer, SideId::Opponent, foe_base, 40.0);
        self.draw_creature(renderer, SideId::Player, my_base, 48.0);
        // Info boxes.
        self.draw_info(renderer, SideId::Opponent, Rect::new(8.0, 8.0, 104.0, 26.0));
        self.draw_info(
            renderer,
            SideId::Player,
            Rect::new(vw - 116.0, field_h - 40.0, 108.0, 34.0),
        );
        self.draw_text_panel(renderer);
        if let Some(menu) = &self.menu {
            self.draw_menu(renderer, menu);
        }
    }

    fn draw_creature(&self, renderer: &mut Renderer, side: SideId, base: Rect, size: f32) {
        let scale = self.sprite_scale(side).clamp(0.0, 1.0);
        if scale <= 0.0 {
            return;
        }
        let Some(member) = self.display.side(side).active_member(0) else {
            return;
        };
        let h = (size * scale).round();
        let x = base.x + (base.width - size) / 2.0;
        let y = base.y + base.height / 2.0 - h;
        if self.slot_fx[side_index(side)].is_some() {
            const AURA: Color = Color {
                r: 255,
                g: 220,
                b: 80,
                a: 200,
            };
            let pad = 4.0;
            renderer.draw_filled_rect(
                Rect::new(x - pad, y - pad, size + pad * 2.0, h + pad * 2.0),
                AURA,
                Layer::Entities,
            );
        }
        renderer.draw_filled_rect(
            Rect::new(x, y, size, h),
            creature_color(&member.name),
            Layer::Entities,
        );
    }

    fn draw_info(&self, renderer: &mut Renderer, side: SideId, rect: Rect) {
        let Some(member) = self.display.side(side).active_member(0) else {
            return;
        };
        self.theme.skin.draw(renderer, rect, PANEL_TINT);
        let style = text_style(Color::WHITE);
        let font = &self.theme.font;
        renderer.draw_text(
            font,
            self.theme.font_tex,
            &member.name,
            rect.x + 6.0,
            rect.y + 4.0,
            style,
        );
        renderer.draw_text(
            font,
            self.theme.font_tex,
            &format!("Lv{}", member.level),
            rect.x + rect.width - 30.0,
            rect.y + 4.0,
            style,
        );
        let bar = Rect::new(rect.x + 6.0, rect.y + 16.0, rect.width - 12.0, 4.0);
        renderer.draw_filled_rect(bar, BAR_BACK, Layer::Ui);
        let frac = member.hp as f32 / member.max_hp.max(1) as f32;
        let fill = Rect::new(bar.x, bar.y, (bar.width * frac).round(), bar.height);
        renderer.draw_filled_rect(fill, hp_color(frac), Layer::Ui);
        if side == SideId::Player {
            renderer.draw_text(
                font,
                self.theme.font_tex,
                &format!("{}/{}", member.hp, member.max_hp),
                rect.x + rect.width - 50.0,
                rect.y + 22.0,
                style,
            );
        }
    }

    fn draw_text_panel(&self, renderer: &mut Renderer) {
        let panel = self.text_panel();
        self.theme.skin.draw(renderer, panel, PANEL_TINT);
        let (tx, ty) = (panel.x + 8.0, panel.y + 6.0);
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
            _ if self.menu.is_some() => {
                let prompt = match self.menu() {
                    Some(MenuKind::Party { forced: true }) => "Choose who goes next.".to_owned(),
                    _ => format!(
                        "What will {} do?",
                        self.display
                            .side(SideId::Player)
                            .active_member(0)
                            .map_or("you", |m| m.name.as_str())
                    ),
                };
                renderer.draw_text(
                    &self.theme.font,
                    self.theme.font_tex,
                    &prompt,
                    tx,
                    ty,
                    text_style(Color::WHITE),
                );
            }
            _ => {}
        }
    }

    fn draw_menu(&self, renderer: &mut Renderer, menu: &OpenMenu) {
        let (vw, vh) = (self.view.0 as f32, self.view.1 as f32);
        let lh = self.theme.font.line_height.max(1) as f32;
        let rows = menu.session.options().len() as f32;
        let height = rows * lh + 12.0;
        let rect = match menu.kind {
            MenuKind::Action => Rect::new(vw - 96.0, vh - height, 96.0, height),
            MenuKind::Fight => Rect::new(vw - 128.0, vh - height, 128.0, height),
            MenuKind::Bag | MenuKind::Party { .. } => Rect::new(8.0, 8.0, vw - 16.0, height),
        };
        self.theme.skin.draw(renderer, rect, PANEL_TINT);
        for (i, option) in menu.session.options().iter().enumerate() {
            let prefix = if menu.session.cursor() == i {
                "> "
            } else {
                "  "
            };
            let tint = if menu.enabled.get(i).copied().unwrap_or(false) {
                Color::WHITE
            } else {
                DISABLED
            };
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                &format!("{prefix}{}", option.label),
                rect.x + 6.0,
                rect.y + 6.0 + lh * i as f32,
                text_style(tint),
            );
        }
    }
}

enum Pick {
    Action(ActionKind),
    Move(u8),
    Item(String),
    Member(PartyIndex),
}

fn take(input: &mut Input, action: BuiltinAction) -> bool {
    let action = Action::Builtin(action);
    let pressed = input.triggered(action);
    if pressed {
        input.consume(action);
    }
    pressed
}

const fn side_index(side: SideId) -> usize {
    match side {
        SideId::Player => 0,
        SideId::Opponent => 1,
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r, g, b, a: 255 }
}

const SKY: Color = rgb(200, 224, 248);
const GROUND: Color = rgb(168, 208, 136);
const PLATFORM: Color = rgb(120, 168, 96);
const BAR_BACK: Color = rgb(56, 56, 64);
const DISABLED: Color = rgb(136, 136, 136);
const PANEL_TINT: Color = Color {
    r: 255,
    g: 255,
    b: 255,
    a: 220,
};

fn text_style(tint: Color) -> TextStyle {
    TextStyle {
        tint,
        scale: 1,
        layer: Layer::Ui,
    }
}

/// Green above half HP, yellow above a fifth, red below.
fn hp_color(frac: f32) -> Color {
    if frac > 0.5 {
        rgb(72, 200, 96)
    } else if frac > 0.2 {
        rgb(232, 192, 48)
    } else {
        rgb(224, 64, 48)
    }
}

/// A stable placeholder colour per creature name.
fn creature_color(name: &str) -> Color {
    let h = name.bytes().fold(0x811c_9dc5_u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(0x0100_0193)
    });
    let [a, b, c, _] = h.to_le_bytes();
    rgb(64 + a % 160, 64 + b % 160, 64 + c % 160)
}
