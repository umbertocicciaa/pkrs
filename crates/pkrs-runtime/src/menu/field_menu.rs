//! Field start menu: party, bag, summary and naming keyboard.

use std::collections::BTreeMap;

use pkrs_data::{GameData, Localizer, Table, TextArgs};
use pkrs_platform::{Action, BuiltinAction, Input};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_script::{FieldActionInfo, ItemUseContext, LuaScriptHost};
use pkrs_ui::{
    BagList, BuiltinItemEffect, ChoiceOption, ChoicePollOutcome, ChoiceSession, ItemUseVerdict,
    KeyboardPollOutcome, NamingKeyboard, PartyReorderSession, builtin_effect, field_use_verdict,
};

use crate::party::PlayerParty;
use crate::ui_theme::UiTheme;

/// Which menu screen is active (for the test harness).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuScreen {
    /// Start menu root.
    Root,
    /// Party list.
    Party,
    /// Bag pocket picker.
    BagPocket,
    /// Items in a pocket.
    BagItems,
    /// Pick party member for an item.
    BagTarget,
    /// Creature summary (`page` 0 = stats, 1 = moves).
    Summary {
        /// Party index.
        member: usize,
        /// Summary page.
        page: usize,
    },
    /// On-screen naming keyboard.
    Naming,
}

enum Screen {
    Root,
    Party,
    BagPocket,
    BagItems,
    BagTarget { item: String },
    Summary { member: usize, page: usize },
    Naming(NamingKeyboard),
}

#[allow(dead_code)]
enum Pending {
    None,
    UseItem { item: String, member: usize },
    FieldAction { id: String },
    OpenNaming,
}

/// Modal start menu over the overworld.
pub struct FieldMenu {
    screen: Screen,
    session: ChoiceSession,
    enabled: Vec<bool>,
    labels: Vec<String>,
    theme: UiTheme,
    #[allow(dead_code)]
    view: (u32, u32),
    text: Localizer,
    party: PlayerParty,
    bag: BTreeMap<String, u32>,
    data: GameData,
    pocket_filter: Option<String>,
    selected_item: Option<String>,
    reorder: PartyReorderSession,
    pending: Pending,
    panel: Rect,
}

impl FieldMenu {
    /// Opens the start menu with the current party and bag.
    pub fn new(
        theme: UiTheme,
        view: (u32, u32),
        text: Localizer,
        party: PlayerParty,
        bag: BTreeMap<String, u32>,
        data: GameData,
    ) -> Result<Self, pkrs_ui::UiError> {
        let margin = 8.0;
        let (vw, vh) = (view.0 as f32, view.1 as f32);
        let panel = Rect::new(margin, margin, vw - 2.0 * margin, vh - 2.0 * margin);
        let mut menu = Self {
            screen: Screen::Root,
            session: ChoiceSession::new(vec![ChoiceOption {
                label: String::new(),
            }])?,
            enabled: Vec::new(),
            labels: Vec::new(),
            theme,
            view,
            text,
            party,
            bag,
            data,
            pocket_filter: None,
            selected_item: None,
            reorder: PartyReorderSession::new(),
            pending: Pending::None,
            panel,
        };
        menu.open_root();
        Ok(menu)
    }

    /// Screen id for scenarios.
    #[must_use]
    pub fn screen(&self) -> MenuScreen {
        match self.screen {
            Screen::Root => MenuScreen::Root,
            Screen::Party => MenuScreen::Party,
            Screen::BagPocket => MenuScreen::BagPocket,
            Screen::BagItems => MenuScreen::BagItems,
            Screen::BagTarget { .. } => MenuScreen::BagTarget,
            Screen::Summary { member, page } => MenuScreen::Summary { member, page },
            Screen::Naming(_) => MenuScreen::Naming,
        }
    }

    /// Visible row labels.
    #[must_use]
    pub fn labels(&self) -> &[String] {
        &self.labels
    }

    /// Highlighted row.
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.session.cursor()
    }

    /// Mutable party (item use, reorder).
    pub fn party_mut(&mut self) -> &mut PlayerParty {
        &mut self.party
    }

    /// Party snapshot.
    #[must_use]
    pub fn party(&self) -> &PlayerParty {
        &self.party
    }

    /// Bag counts.
    #[must_use]
    pub fn bag(&self) -> &BTreeMap<String, u32> {
        &self.bag
    }

    fn say(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut ta = TextArgs::new();
        for (k, v) in args {
            ta = ta.with(*k, *v);
        }
        self.text.key(key, &ta)
    }

    fn item_name(&self, id: &str) -> String {
        Table::by_id(self.data.items(), id)
            .and_then(|i| {
                if i.name.is_empty() {
                    None
                } else {
                    self.text
                        .resolve(&i.name, &TextArgs::new())
                        .or_else(|| Some(i.name.clone()))
                }
            })
            .unwrap_or_else(|| display_name(id))
    }

    fn pocket_of(&self, id: &str) -> String {
        Table::by_id(self.data.items(), id)
            .map(|i| i.pocket.clone())
            .unwrap_or_else(|| "items".into())
    }

    fn open_root(&mut self) {
        self.screen = Screen::Root;
        self.pending = Pending::None;
        let labels = vec![
            self.say("menu.party", &[]),
            self.say("menu.bag", &[]),
            self.say("menu.close", &[]),
        ];
        self.set_list(labels, vec![true; 3]);
    }

    fn open_party(&mut self, field_actions: &[FieldActionInfo]) {
        self.screen = Screen::Party;
        let mut labels = Vec::new();
        let mut enabled = Vec::new();
        for m in self.party.members() {
            let species = Table::by_id(self.data.species(), &m.species);
            let (hp, max) = species.map_or((0, 0), |s| (m.current_hp(s), m.stats(s).max_hp));
            let name = m.name();
            let level = m.level.to_string();
            let hp_s = hp.to_string();
            let max_s = max.to_string();
            labels.push(self.say(
                "menu.party.member",
                &[
                    ("name", name.as_str()),
                    ("level", level.as_str()),
                    ("hp", hp_s.as_str()),
                    ("max_hp", max_s.as_str()),
                ],
            ));
            enabled.push(true);
        }
        if self.reorder.mode() != pkrs_ui::PartyReorderMode::Browse {
            labels.push(self.say("menu.party.swap_done", &[]));
            enabled.push(true);
        } else {
            labels.push(self.say("menu.party.reorder", &[]));
            enabled.push(self.party.members().len() > 1);
            for action in field_actions {
                let label = if let Some(key) = action.label.strip_prefix("text:") {
                    self.text
                        .resolve(key, &TextArgs::new())
                        .unwrap_or_else(|| action.label.clone())
                } else {
                    action.label.clone()
                };
                labels.push(label);
                enabled.push(true);
            }
        }
        labels.push(self.say("menu.back", &[]));
        enabled.push(true);
        self.set_list(labels, enabled);
    }

    fn open_bag_pockets(&mut self) {
        self.screen = Screen::BagPocket;
        let list = BagList::new(&self.bag, |id| self.pocket_of(id));
        let pockets = list.pockets();
        let mut labels: Vec<String> = pockets
            .iter()
            .map(|p| self.say("menu.bag.pocket", &[("pocket", p)]))
            .collect();
        let mut enabled = vec![true; labels.len()];
        labels.push(self.say("menu.back", &[]));
        enabled.push(true);
        self.pocket_filter = None;
        self.set_list(labels, enabled);
    }

    fn open_bag_items(&mut self, pocket: &str) {
        self.screen = Screen::BagItems;
        self.pocket_filter = Some(pocket.to_owned());
        let mut list = BagList::new(&self.bag, |id| self.pocket_of(id));
        list.set_pocket_filter(Some(pocket.to_owned()));
        let mut labels: Vec<String> = list
            .visible()
            .iter()
            .map(|e| {
                let item = self.item_name(&e.id);
                let count = e.count.to_string();
                self.say(
                    "menu.bag.item",
                    &[("item", item.as_str()), ("count", count.as_str())],
                )
            })
            .collect();
        let mut enabled = vec![true; labels.len()];
        labels.push(self.say("menu.back", &[]));
        enabled.push(true);
        self.set_list(labels, enabled);
    }

    fn open_bag_target(&mut self, item: &str, host: &LuaScriptHost) {
        self.screen = Screen::BagTarget {
            item: item.to_owned(),
        };
        self.selected_item = Some(item.to_owned());
        let pocket = self.pocket_of(item);
        let has_hook = host.has_item_use_hook(item);
        let mut labels = Vec::new();
        let mut enabled = Vec::new();
        for (i, m) in self.party.members().iter().enumerate() {
            let _ = i;
            let species = Table::by_id(self.data.species(), &m.species);
            let (hp, max) = species.map_or((0, 0), |s| (m.current_hp(s), m.stats(s).max_hp));
            let name = m.name();
            let level = m.level.to_string();
            let hp_s = hp.to_string();
            let max_s = max.to_string();
            labels.push(self.say(
                "menu.party.member",
                &[
                    ("name", name.as_str()),
                    ("level", level.as_str()),
                    ("hp", hp_s.as_str()),
                    ("max_hp", max_s.as_str()),
                ],
            ));
            let verdict = field_use_verdict(&pocket, item, hp, max, has_hook);
            enabled.push(verdict == ItemUseVerdict::Allowed);
        }
        labels.push(self.say("menu.back", &[]));
        enabled.push(true);
        self.set_list(labels, enabled);
    }

    fn open_summary(&mut self, member: usize, page: usize) {
        self.screen = Screen::Summary { member, page };
        let Some(m) = self.party.member(member) else {
            self.open_root();
            return;
        };
        let species = self.data.species().by_id(&m.species);
        let stats = species.map(|s| m.stats(s));
        let name = m.name();
        let level = m.level.to_string();
        let labels = if page == 0 {
            let mut rows = vec![self.say(
                "menu.summary.info",
                &[("name", name.as_str()), ("level", level.as_str())],
            )];
            if let Some(st) = stats {
                let hp = species.map_or(0, |s| m.current_hp(s));
                let hp_s = hp.to_string();
                let max_s = st.max_hp.to_string();
                let atk = st.attack.to_string();
                let def = st.defense.to_string();
                let spd = st.speed.to_string();
                rows.push(self.say(
                    "menu.summary.hp",
                    &[("hp", hp_s.as_str()), ("max", max_s.as_str())],
                ));
                rows.push(self.say(
                    "menu.summary.stat",
                    &[("stat", "ATK"), ("value", atk.as_str())],
                ));
                rows.push(self.say(
                    "menu.summary.stat",
                    &[("stat", "DEF"), ("value", def.as_str())],
                ));
                rows.push(self.say(
                    "menu.summary.stat",
                    &[("stat", "SPD"), ("value", spd.as_str())],
                ));
            }
            rows.push(self.say("menu.summary.moves_page", &[]));
            rows.push(self.say("menu.back", &[]));
            rows
        } else {
            let mut rows: Vec<String> = m
                .moves
                .iter()
                .map(|mv| {
                    let label = display_name(mv);
                    self.say("menu.summary.move", &[("move", label.as_str())])
                })
                .collect();
            if rows.is_empty() {
                rows.push(self.say("menu.summary.no_moves", &[]));
            }
            rows.push(self.say("menu.summary.stats_page", &[]));
            rows.push(self.say("menu.naming", &[]));
            rows.push(self.say("menu.back", &[]));
            rows
        };
        let n = labels.len();
        self.set_list(labels, vec![true; n]);
    }

    fn open_naming(&mut self) -> Result<(), pkrs_ui::UiError> {
        self.screen = Screen::Naming(NamingKeyboard::new(10));
        self.labels = vec![self.say("menu.naming.title", &[])];
        self.enabled = vec![];
        Ok(())
    }

    #[allow(clippy::expect_used)] // empty fallback list is always valid
    fn set_list(&mut self, labels: Vec<String>, enabled: Vec<bool>) {
        let options = labels
            .iter()
            .map(|l| ChoiceOption { label: l.clone() })
            .collect();
        self.labels = labels;
        self.enabled = enabled;
        self.session = ChoiceSession::new(options).unwrap_or_else(|_| {
            ChoiceSession::new(vec![ChoiceOption {
                label: String::new(),
            }])
            .expect("fallback choice list")
        });
    }

    fn pop_screen(&mut self, host: &LuaScriptHost, field_actions: &[FieldActionInfo]) {
        match self.screen {
            Screen::Root => {}
            Screen::Party => self.open_root(),
            Screen::BagPocket => self.open_root(),
            Screen::BagItems => self.open_bag_pockets(),
            Screen::BagTarget { .. } => {
                if let Some(pocket) = self.pocket_filter.clone() {
                    self.open_bag_items(&pocket);
                } else {
                    self.open_bag_pockets();
                }
            }
            Screen::Summary { member, page } => {
                if page == 1 {
                    self.open_summary(member, 0);
                } else {
                    self.open_party(field_actions);
                }
            }
            Screen::Naming(_) => self.open_root(),
        }
        let _ = host;
    }

    fn apply_builtin_item(&mut self, item: &str, member: usize) -> bool {
        let Some(BuiltinItemEffect::Heal(amount)) = builtin_effect(item) else {
            return false;
        };
        let Some(m) = self.party.member_mut(member) else {
            return false;
        };
        let Some(species) = self.data.species().by_id(&m.species).cloned() else {
            return false;
        };
        m.heal(amount, &species);
        if let Some(c) = self.bag.get_mut(item) {
            if *c == 0 {
                return false;
            }
            *c = c.saturating_sub(1);
            if *c == 0 {
                self.bag.remove(item);
            }
            true
        } else {
            false
        }
    }

    /// One tick of input; `true` when the menu should close (root cancel).
    pub fn update(
        &mut self,
        input: &mut Input,
        host: &mut LuaScriptHost,
        field_actions: &[FieldActionInfo],
    ) -> bool {
        if let Screen::Naming(ref mut kb) = self.screen {
            if let Some(outcome) = kb.poll(input) {
                match outcome {
                    KeyboardPollOutcome::Done(name) => {
                        tracing::info!(target: "menu", %name, "naming confirmed");
                        self.open_root();
                    }
                    KeyboardPollOutcome::Cancelled => self.open_root(),
                }
            }
            return false;
        }
        let Some(outcome) = self.session.poll(input) else {
            return false;
        };
        match outcome {
            ChoicePollOutcome::Cancelled => {
                input.consume(Action::Builtin(BuiltinAction::Cancel));
                if matches!(self.screen, Screen::Root) {
                    return true;
                }
                self.pop_screen(host, field_actions);
            }
            ChoicePollOutcome::Picked(ix) => {
                if !self.enabled.get(ix).copied().unwrap_or(false) {
                    return false;
                }
                input.consume(Action::Builtin(BuiltinAction::Confirm));
                match &self.screen {
                    Screen::Root => match ix {
                        0 => self.open_party(field_actions),
                        1 => self.open_bag_pockets(),
                        2 => return true,
                        _ => {}
                    },
                    Screen::Party => {
                        let members = self.party.members().len();
                        if ix < members {
                            if let Some((a, b)) = self.reorder.confirm(ix) {
                                self.party.swap_members(a, b);
                                self.reorder.cancel();
                            } else {
                                self.open_summary(ix, 0);
                            }
                        } else if ix == members {
                            self.reorder.toggle_reorder();
                            self.open_party(field_actions);
                        } else if ix == self.labels.len() - 1 {
                            self.open_root();
                        } else {
                            let action_ix = ix - members - 1;
                            if let Some(action) = field_actions.get(action_ix) {
                                self.pending = Pending::FieldAction {
                                    id: action.id.clone(),
                                };
                                return true;
                            }
                        }
                    }
                    Screen::BagPocket => {
                        let list = BagList::new(&self.bag, |id| self.pocket_of(id));
                        let pockets = list.pockets();
                        if ix < pockets.len() {
                            self.open_bag_items(&pockets[ix]);
                        } else {
                            self.open_root();
                        }
                    }
                    Screen::BagItems => {
                        let mut list = BagList::new(&self.bag, |id| self.pocket_of(id));
                        if let Some(pocket) = &self.pocket_filter {
                            list.set_pocket_filter(Some(pocket.clone()));
                        }
                        let visible: Vec<_> =
                            list.visible().into_iter().map(|e| e.id.clone()).collect();
                        if ix < visible.len() {
                            self.open_bag_target(&visible[ix], host);
                        } else if let Some(pocket) = self.pocket_filter.clone() {
                            let _ = pocket;
                            self.open_bag_pockets();
                        }
                    }
                    Screen::BagTarget { item } => {
                        let item = item.clone();
                        let members = self.party.members().len();
                        if ix < members {
                            let ctx = ItemUseContext {
                                item_id: item.clone(),
                                party_index: ix,
                            };
                            let used = if host.run_item_use(&ctx) {
                                true
                            } else {
                                self.apply_builtin_item(&item, ix)
                            };
                            if used {
                                self.pending = Pending::UseItem { item, member: ix };
                                return true;
                            }
                        } else if let Some(pocket) = self.pocket_filter.clone() {
                            self.open_bag_items(&pocket);
                        }
                    }
                    Screen::Summary { member, page } => {
                        let last = self.labels.len().saturating_sub(1);
                        if ix == last {
                            self.open_party(field_actions);
                        } else if *page == 0 && ix == self.labels.len().saturating_sub(2) {
                            self.open_summary(*member, 1);
                        } else if *page == 1 {
                            if ix == self.labels.len().saturating_sub(3) {
                                self.open_summary(*member, 0);
                            } else if ix == self.labels.len().saturating_sub(2) {
                                let _ = self.open_naming();
                            }
                        }
                    }
                    Screen::Naming(_) => {}
                }
            }
        }
        false
    }

    /// Field action chosen from the party menu (`Pending::FieldAction`).
    #[must_use]
    pub fn take_field_action(&mut self) -> Option<String> {
        match std::mem::replace(&mut self.pending, Pending::None) {
            Pending::FieldAction { id } => Some(id),
            other => {
                self.pending = other;
                None
            }
        }
    }

    fn scale_f(scale: u32) -> f32 {
        scale.max(1) as f32
    }

    /// Draws the menu panel.
    pub fn render(&self, renderer: &mut Renderer) {
        let tint = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 220,
        };
        self.theme.skin.draw(renderer, self.panel, tint);
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        let lh = self.theme.font.line_height as f32 * Self::scale_f(style.scale);
        let title = match self.screen {
            Screen::Root => self.say("menu.title", &[]),
            Screen::Party => self.say("menu.party.title", &[]),
            Screen::BagPocket | Screen::BagItems | Screen::BagTarget { .. } => {
                self.say("menu.bag.title", &[])
            }
            Screen::Summary { .. } => self.say("menu.summary.title", &[]),
            Screen::Naming(_) => self.say("menu.naming.title", &[]),
        };
        renderer.draw_text(
            &self.theme.font,
            self.theme.font_tex,
            &title,
            self.panel.x + 12.0,
            self.panel.y + 8.0,
            style,
        );
        if let Screen::Naming(ref kb) = self.screen {
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                kb.text(),
                self.panel.x + 12.0,
                self.panel.y + 8.0 + lh,
                style,
            );
            let (row, col) = kb.cursor();
            let base_x = self.panel.x + 12.0;
            let base_y = self.panel.y + 8.0 + lh * 3.0;
            for (r, keys) in pkrs_ui::ROWS.iter().enumerate() {
                for (c, key) in keys.iter().enumerate() {
                    let x = base_x + c as f32 * 24.0;
                    let y = base_y + r as f32 * lh;
                    let prefix = if row == r && col == c { ">" } else { " " };
                    renderer.draw_text(
                        &self.theme.font,
                        self.theme.font_tex,
                        &format!("{prefix}{key}"),
                        x,
                        y,
                        style,
                    );
                }
            }
            return;
        }
        let base_x = self.panel.x + 28.0;
        let base_y = self.panel.y + 12.0 + lh;
        for (i, label) in self.labels.iter().enumerate() {
            let y = base_y + lh * i as f32;
            let prefix = if self.session.cursor() == i {
                "> "
            } else {
                "  "
            };
            let muted = !self.enabled.get(i).copied().unwrap_or(true);
            let mut row_style = style;
            if muted {
                row_style.tint = Color {
                    r: 160,
                    g: 160,
                    b: 160,
                    a: 255,
                };
            }
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                prefix,
                base_x - 18.0,
                y,
                row_style,
            );
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                label,
                base_x,
                y,
                row_style,
            );
        }
    }
}

fn display_name(id: &str) -> String {
    id.split('_')
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
