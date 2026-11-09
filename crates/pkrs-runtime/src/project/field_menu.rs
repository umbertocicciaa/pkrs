//! Start menu integration on [`ProjectHost`](super::ProjectHost) (issue #57, §4.1).

use pkrs_platform::{Action, BuiltinAction};

use crate::menu::FieldMenu;

use super::ProjectHost;

impl ProjectHost {
    /// The open field start menu, if any.
    #[must_use]
    pub fn field_menu(&self) -> Option<&FieldMenu> {
        self.field_menu.as_ref()
    }

    /// Opens the start menu (tests that drive the stack without the menu key edge).
    pub fn open_field_menu(&mut self) -> Result<(), String> {
        self.try_open_field_menu()
    }

    /// Sets a party member's field HP (tests and tooling).
    pub fn set_party_hp(&mut self, index: usize, hp: u32) -> Result<(), String> {
        self.ensure_session_party()?;
        let content = self
            .battle_content
            .as_ref()
            .ok_or_else(|| "battle content missing".to_owned())?;
        let party = self.party.as_mut().ok_or_else(|| "no party".to_owned())?;
        let member = party
            .member_mut(index)
            .ok_or_else(|| format!("party index {index} out of range"))?;
        let species = content
            .pack()
            .species
            .get(&member.species)
            .ok_or_else(|| format!("unknown species `{}`", member.species))?;
        member.set_hp(hp, species);
        Ok(())
    }

    pub(super) fn ensure_session_party(&mut self) -> Result<(), String> {
        if self.party.is_some() {
            return Ok(());
        }
        self.ensure_battle_content()?;
        let content = self
            .battle_content
            .as_ref()
            .ok_or_else(|| "battle content missing".to_owned())?;
        let side = content.player_party_side().map_err(|e| e.to_string())?;
        self.party = Some(crate::party::PlayerParty::from_side(&side, content.pack()));
        Ok(())
    }

    pub(super) fn tick_field_menu_if_open(&mut self) -> bool {
        if self.field_menu.is_none() {
            return false;
        }
        self.tick_field_menu();
        true
    }

    pub(super) fn try_menu_hotkey(&mut self) {
        let menu_key = Action::Builtin(BuiltinAction::Menu);
        if !self.walker.is_idle() {
            return;
        }
        if self.scripts.as_ref().is_some_and(|s| s.player_locked()) {
            return;
        }
        if !self.input.triggered(menu_key) {
            return;
        }
        self.input.consume(menu_key);
        if let Err(err) = self.try_open_field_menu() {
            tracing::debug!(target: "menu", %err, "start menu not opened");
        }
    }

    pub(super) fn render_field_menu_overlay(&mut self) {
        if let Some(menu) = &self.field_menu {
            menu.render(&mut self.renderer);
        }
    }

    fn try_open_field_menu(&mut self) -> Result<(), String> {
        if self.battle.is_some() || self.overlay.is_some() || self.is_warping() {
            return Err("menu unavailable".to_owned());
        }
        let Some(scripts) = self.scripts.as_ref() else {
            return Err("scripting is disabled".to_owned());
        };
        if scripts.player_locked() {
            return Err("player is locked".to_owned());
        }
        let theme = self
            .ui_theme
            .clone()
            .ok_or_else(|| "no UI theme".to_owned())?;
        let bag = scripts.world().get().state.bag().clone();
        self.ensure_session_party()?;
        let party = self.party.clone().ok_or_else(|| "no party".to_owned())?;
        let data = self.ensure_game_data()?.clone();
        let view = self.renderer.view_size();
        let mut menu = FieldMenu::new(theme, view, self.localizer.clone(), party, bag, data)
            .map_err(|e| e.to_string())?;
        menu.set_options(self.user_config.clone(), self.project.display().scale);
        self.field_menu = Some(menu);
        Ok(())
    }

    fn commit_field_menu(&mut self, menu: &FieldMenu) {
        self.party = Some(menu.party().clone());
        if let Some(scripts) = self.scripts.as_mut() {
            scripts
                .world()
                .get_mut()
                .state
                .replace_bag(menu.bag().clone());
        }
    }

    fn tick_field_menu(&mut self) {
        let Some(mut menu) = self.field_menu.take() else {
            return;
        };
        let close = match self.scripts.as_mut() {
            None => {
                self.field_menu = Some(menu);
                return;
            }
            Some(scripts) => {
                let field_actions = scripts.party_field_actions();
                let host = scripts.host_mut();
                menu.update(&mut self.input, host, &field_actions)
            }
        };
        if let Some(config) = menu.take_saved_options() {
            self.persist_user_config(config);
        }
        if close {
            let action = menu.take_field_action();
            self.commit_field_menu(&menu);
            self.field_menu = None;
            if let Some(id) = action {
                if let Some(scripts) = self.scripts.as_mut() {
                    scripts.use_field_action(&id);
                }
            }
            return;
        }
        self.field_menu = Some(menu);
    }
}
