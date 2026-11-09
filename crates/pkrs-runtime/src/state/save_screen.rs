//! Save slot picker (§4.1, §12): headers only, load/save and overwrite confirm.

use pkrs_platform::{Action, BuiltinAction};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_save::{SaveSlot, SlotInfo, slot_screen_detail, slot_screen_label};
use pkrs_ui::{ChoiceOption, ChoicePollOutcome, ChoiceSession};

use super::{GameState, Transition};
use crate::ctx::{BootAction, RuntimeCtx};
use crate::ui_theme::UiTheme;

/// Load a save from the title screen or save the running game from a menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveScreenMode {
    /// Continue: pick a non-empty slot and confirm.
    Load,
    /// Save: pick a slot; occupied slots ask to overwrite.
    #[expect(dead_code, reason = "in-game save menu (#57)")]
    Save,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    PickSlot,
    ConfirmBackup,
    ConfirmOverwrite,
    Info,
}

/// Built-in save slot list (`SaveScreen`, §4.1).
pub struct SaveScreenState {
    mode: SaveScreenMode,
    theme: UiTheme,
    panel: Rect,
    slots: Vec<(SaveSlot, SlotInfo)>,
    session: ChoiceSession,
    phase: Phase,
    picked: usize,
    info_ticks: u32,
    confirm_prompt: Option<String>,
}

impl SaveScreenState {
    /// Lists `slots` (from [`SaveStore::list`](pkrs_save::SaveStore::list)).
    pub fn new(
        mode: SaveScreenMode,
        theme: UiTheme,
        renderer: &Renderer,
        slots: Vec<(SaveSlot, SlotInfo)>,
    ) -> Result<Self, pkrs_ui::UiError> {
        let rows: Vec<ChoiceOption> = slots
            .iter()
            .map(|(slot, info)| ChoiceOption {
                label: slot_screen_label(*slot, info),
            })
            .collect();
        if rows.is_empty() {
            return Err(pkrs_ui::UiError::EmptyChoice);
        }
        let (vw, vh) = renderer.view_size();
        let margin = 8.0f32;
        let wf = vw as f32;
        let vf = vh as f32;
        let panel_h = (vf * 0.62).floor();
        let panel = Rect::new(margin, margin, wf - 2.0 * margin, panel_h);
        let session = ChoiceSession::new(rows)?;
        Ok(Self {
            mode,
            theme,
            panel,
            slots,
            session,
            phase: Phase::PickSlot,
            picked: 0,
            info_ticks: 0,
            confirm_prompt: None,
        })
    }

    fn detail_line(&self, index: usize) -> Option<String> {
        let line = slot_screen_detail(&self.slots[index].1);
        (!line.is_empty()).then_some(line)
    }

    fn slot_ok_to_pick(&self, index: usize) -> bool {
        match (&self.mode, &self.slots[index].1) {
            (SaveScreenMode::Load, SlotInfo::Saved { .. }) => true,
            (SaveScreenMode::Load, SlotInfo::Empty | SlotInfo::Corrupt(_)) => false,
            (SaveScreenMode::Save, SlotInfo::Corrupt(_)) => false,
            (SaveScreenMode::Save, SlotInfo::Empty | SlotInfo::Saved { .. }) => true,
        }
    }

    fn yes_no_options() -> Vec<ChoiceOption> {
        vec![
            ChoiceOption {
                label: "Yes".into(),
            },
            ChoiceOption { label: "No".into() },
        ]
    }
}

impl GameState for SaveScreenState {
    fn id(&self) -> &str {
        "save"
    }

    fn is_opaque(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        match self.phase {
            Phase::Info => {
                if ctx.input.triggered(Action::Builtin(BuiltinAction::Confirm)) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Confirm));
                    self.phase = Phase::PickSlot;
                    return Transition::None;
                }
                self.info_ticks = self.info_ticks.saturating_add(1);
                if self.info_ticks > 90 {
                    self.phase = Phase::PickSlot;
                }
                Transition::None
            }
            Phase::PickSlot => {
                if let Some(ChoicePollOutcome::Cancelled) = self.session.poll(&ctx.input) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Cancel));
                    return Transition::Pop;
                }
                if let Some(ChoicePollOutcome::Picked(ix)) = self.session.poll(&ctx.input) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Confirm));
                    if !self.slot_ok_to_pick(ix) {
                        self.phase = Phase::Info;
                        self.info_ticks = 0;
                        return Transition::None;
                    }
                    self.picked = ix;
                    let slot = self.slots[ix].0;
                    match (self.mode, &self.slots[ix].1) {
                        (
                            SaveScreenMode::Load,
                            SlotInfo::Saved {
                                from_backup: false, ..
                            },
                        ) => {
                            ctx.request_boot(BootAction::LoadSlot(slot));
                            return Transition::Pop;
                        }
                        (
                            SaveScreenMode::Load,
                            SlotInfo::Saved {
                                from_backup: true, ..
                            },
                        ) => {
                            self.phase = Phase::ConfirmBackup;
                            self.confirm_prompt = Some("Load backup save?".into());
                            let Ok(session) = ChoiceSession::new(Self::yes_no_options()) else {
                                return Transition::None;
                            };
                            self.session = session;
                        }
                        (SaveScreenMode::Load, _) => {
                            self.phase = Phase::Info;
                            self.info_ticks = 0;
                        }
                        (SaveScreenMode::Save, SlotInfo::Saved { .. }) => {
                            self.phase = Phase::ConfirmOverwrite;
                            self.confirm_prompt = Some(format!("Overwrite {slot}?"));
                            let Ok(session) = ChoiceSession::new(Self::yes_no_options()) else {
                                return Transition::None;
                            };
                            self.session = session;
                        }
                        (SaveScreenMode::Save, SlotInfo::Empty) => {
                            ctx.request_boot(BootAction::SaveSlot(slot));
                            return Transition::Pop;
                        }
                        (SaveScreenMode::Save, SlotInfo::Corrupt(_)) => {
                            self.phase = Phase::Info;
                            self.info_ticks = 0;
                        }
                    }
                }
                Transition::None
            }
            Phase::ConfirmBackup => {
                if let Some(ChoicePollOutcome::Cancelled) = self.session.poll(&ctx.input) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Cancel));
                    self.rebuild_pick_list(ctx);
                    return Transition::None;
                }
                if let Some(ChoicePollOutcome::Picked(ix)) = self.session.poll(&ctx.input) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Confirm));
                    if ix == 0 {
                        let slot = self.slots[self.picked].0;
                        ctx.request_boot(BootAction::LoadSlot(slot));
                        return Transition::Pop;
                    }
                    self.rebuild_pick_list(ctx);
                }
                Transition::None
            }
            Phase::ConfirmOverwrite => {
                if let Some(ChoicePollOutcome::Cancelled) = self.session.poll(&ctx.input) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Cancel));
                    self.rebuild_pick_list(ctx);
                    return Transition::None;
                }
                if let Some(ChoicePollOutcome::Picked(ix)) = self.session.poll(&ctx.input) {
                    ctx.input.consume(Action::Builtin(BuiltinAction::Confirm));
                    if ix == 0 {
                        let slot = self.slots[self.picked].0;
                        ctx.request_boot(BootAction::SaveSlot(slot));
                        return Transition::Pop;
                    }
                    self.rebuild_pick_list(ctx);
                }
                Transition::None
            }
        }
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        let tint = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 190,
        };
        self.theme.skin.draw(renderer, self.panel, tint);
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        let lh = self.theme.font.line_height as f32;
        let base_x = self.panel.x + 20.0;
        let mut base_y = self.panel.y + 10.0;
        let heading = match (self.mode, self.phase) {
            (SaveScreenMode::Load, Phase::PickSlot) => "Continue",
            (SaveScreenMode::Save, Phase::PickSlot) => "Save",
            (_, Phase::ConfirmBackup) => "Backup",
            (_, Phase::ConfirmOverwrite) => "Overwrite",
            (_, Phase::Info) => "Save",
        };
        renderer.draw_text(
            &self.theme.font,
            self.theme.font_tex,
            heading,
            base_x,
            base_y,
            style,
        );
        base_y += lh + 4.0;
        if let Some(prompt) = self.confirm_prompt.as_deref() {
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                prompt,
                base_x,
                base_y,
                style,
            );
            base_y += lh + 4.0;
        }
        if self.phase == Phase::Info {
            let msg = match (self.mode, self.slots.get(self.session.cursor())) {
                (SaveScreenMode::Load, Some((_, SlotInfo::Empty))) => "No save in this slot.",
                (SaveScreenMode::Load, Some((_, SlotInfo::Corrupt(_)))) => "Save file is corrupt.",
                (SaveScreenMode::Save, Some((_, SlotInfo::Corrupt(_)))) => "Save file is corrupt.",
                _ => "Cannot use this slot.",
            };
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                msg,
                base_x,
                base_y,
                style,
            );
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                "Press Confirm",
                base_x,
                base_y + lh + 8.0,
                style,
            );
            return;
        }
        for (i, o) in self.session.options().iter().enumerate() {
            let y = base_y + lh * i as f32;
            let prefix = if self.session.cursor() == i {
                "> "
            } else {
                "  "
            };
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                prefix,
                base_x - 14.0,
                y,
                style,
            );
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                &o.label,
                base_x,
                y,
                style,
            );
            if self.phase == Phase::PickSlot {
                if let Some(detail) = self.detail_line(i) {
                    renderer.draw_text(
                        &self.theme.font,
                        self.theme.font_tex,
                        &detail,
                        base_x + 8.0,
                        y + lh * 0.85,
                        TextStyle {
                            tint: Color {
                                r: 200,
                                g: 200,
                                b: 200,
                                a: 255,
                            },
                            ..style
                        },
                    );
                }
            }
        }
    }
}

impl SaveScreenState {
    fn rebuild_pick_list(&mut self, _ctx: &mut RuntimeCtx) {
        let rows: Vec<ChoiceOption> = self
            .slots
            .iter()
            .map(|(slot, info)| ChoiceOption {
                label: slot_screen_label(*slot, info),
            })
            .collect();
        if let Ok(session) = ChoiceSession::new(rows) {
            self.session = session;
            self.phase = Phase::PickSlot;
            self.confirm_prompt = None;
            self.session
                .set_cursor(self.picked.min(self.slots.len().saturating_sub(1)));
        }
    }
}

/// Whether any numbered or autosave slot has loadable header data.
#[must_use]
pub fn any_loadable_save(slots: &[(SaveSlot, SlotInfo)]) -> bool {
    slots
        .iter()
        .any(|(_, info)| matches!(info, SlotInfo::Saved { .. }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_save::{SaveData, SaveLoader, SaveStore};

    #[test]
    fn slot_labels_follow_store_ordering() {
        let store = SaveStore::with_fs("saves", Box::new(pkrs_save::StdFs));
        let mut save = SaveData::default();
        save.header.summary.player_name = "Asha".into();
        store.write(SaveSlot::Slot(2), &save).unwrap();
        let list = store.list(&SaveLoader::new());
        assert!(any_loadable_save(&list));
        let label = slot_screen_label(list[1].0, &list[1].1);
        assert!(label.contains("Asha"));
    }
}
