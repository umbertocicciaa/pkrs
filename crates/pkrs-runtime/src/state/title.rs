//! Title screen: new game, continue and quit (§4.1).

use pkrs_platform::{Action, BuiltinAction};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_save::SaveLoader;
use pkrs_ui::{ChoiceOption, ChoicePollOutcome, ChoiceSession};

use super::save_screen::{SaveScreenMode, SaveScreenState, any_loadable_save};
use super::{GameState, Transition};
use crate::ctx::{BootAction, RuntimeCtx};
use crate::project::{Project, save_store};
use crate::ui_theme::UiTheme;

/// Built-in title state (§4.1).
pub struct TitleState {
    game_name: String,
    theme: UiTheme,
    panel: Rect,
    session: ChoiceSession,
    has_saves: bool,
    continue_screen: Option<SaveScreenState>,
}

impl TitleState {
    /// Title menu for `project` (reads save headers for Continue).
    pub fn for_project(
        project: &Project,
        theme: UiTheme,
        renderer: &Renderer,
    ) -> Result<Self, pkrs_ui::UiError> {
        let slots = save_store(&project.paths).list(&SaveLoader::new());
        let has_saves = any_loadable_save(&slots);
        let continue_screen = if has_saves {
            let load_slots: Vec<_> = slots
                .into_iter()
                .filter(|(_, info)| matches!(info, pkrs_save::SlotInfo::Saved { .. }))
                .collect();
            Some(SaveScreenState::new(
                SaveScreenMode::Load,
                theme.clone(),
                renderer,
                load_slots,
            )?)
        } else {
            None
        };
        let mut options = vec![ChoiceOption {
            label: "New Game".into(),
        }];
        if has_saves {
            options.push(ChoiceOption {
                label: "Continue".into(),
            });
        }
        options.push(ChoiceOption {
            label: "Quit".into(),
        });
        let (vw, vh) = renderer.view_size();
        let margin = 8.0f32;
        let wf = vw as f32;
        let vf = vh as f32;
        let panel_h = (vf * 0.42).floor();
        let panel = Rect::new(margin, vf * 0.22, wf - 2.0 * margin, panel_h);
        Ok(Self {
            game_name: project.manifest.game.name.clone(),
            theme,
            panel,
            session: ChoiceSession::new(options)?,
            has_saves,
            continue_screen,
        })
    }

    /// Minimal title menu for [`crate::RuntimeHost`] stack integration tests (no project).
    pub fn for_stack_demo(theme: UiTheme, renderer: &Renderer) -> Result<Self, pkrs_ui::UiError> {
        let options = vec![ChoiceOption {
            label: "Quit".into(),
        }];
        let (vw, vh) = renderer.view_size();
        let margin = 8.0f32;
        let wf = vw as f32;
        let vf = vh as f32;
        let panel_h = (vf * 0.42).floor();
        let panel = Rect::new(margin, vf * 0.22, wf - 2.0 * margin, panel_h);
        Ok(Self {
            game_name: "pkrs".into(),
            theme,
            panel,
            session: ChoiceSession::new(options)?,
            has_saves: false,
            continue_screen: None,
        })
    }
}

impl GameState for TitleState {
    fn id(&self) -> &str {
        "title"
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        if let Some(ChoicePollOutcome::Picked(ix)) = self.session.poll(&ctx.input) {
            ctx.input.consume(Action::Builtin(BuiltinAction::Confirm));
            let label = &self.session.options()[ix].label;
            return match label.as_str() {
                "New Game" => {
                    ctx.request_boot(BootAction::NewGame);
                    Transition::Pop
                }
                "Continue" => {
                    if let Some(screen) = self.continue_screen.take() {
                        return Transition::Push(Box::new(screen));
                    }
                    Transition::None
                }
                "Quit" => Transition::Quit,
                _ => Transition::None,
            };
        }
        Transition::None
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        let (vw, vh) = renderer.view_size();
        renderer.draw_filled_rect(
            Rect::new(0.0, 0.0, vw as f32, vh as f32),
            Color {
                r: 16,
                g: 20,
                b: 48,
                a: 255,
            },
            Layer::Ui,
        );
        let lh = self.theme.font.line_height as f32;
        renderer.draw_text(
            &self.theme.font,
            self.theme.font_tex,
            &self.game_name,
            vw as f32 * 0.5 - self.game_name.len() as f32 * 3.0,
            vh as f32 * 0.12,
            style,
        );
        let tint = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 200,
        };
        self.theme.skin.draw(renderer, self.panel, tint);
        let base_x = self.panel.x + 28.0;
        let base_y = self.panel.y + 14.0;
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
                base_x - 18.0,
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
        }
        if !self.has_saves {
            renderer.draw_text(
                &self.theme.font,
                self.theme.font_tex,
                "No saves yet",
                base_x,
                base_y + lh * 3.2,
                TextStyle {
                    tint: Color {
                        r: 180,
                        g: 180,
                        b: 200,
                        a: 255,
                    },
                    ..style
                },
            );
        }
    }
}
