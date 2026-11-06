//! Choice menu overlay (§4.1, §5.5).

use std::cell::Cell;
use std::rc::Rc;

use pkrs_platform::{Action, BuiltinAction};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_ui::{ChoiceOption, ChoicePollOutcome, ChoiceSession};

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;
use crate::ui_theme::UiTheme;

#[inline]
fn scale_f(scale: u32) -> f32 {
    scale.max(1) as f32
}

#[inline]
fn u32_px(v: u32) -> f32 {
    v as f32
}

/// Non-opaque choice list over the scene.
///
/// `result` is set before [`Transition::Pop`]: `None` = not finished, `Some(Some(i))` picked,
/// `Some(None)` cancelled.
pub struct ChoiceState {
    session: ChoiceSession,
    theme: UiTheme,
    panel: Rect,
    result: Rc<Cell<Option<Option<usize>>>>,
}

impl ChoiceState {
    /// Overlays `options`; writes pick or cancel into `result` before popping.
    pub fn new(
        theme: UiTheme,
        renderer: &Renderer,
        options: Vec<ChoiceOption>,
        result: Rc<Cell<Option<Option<usize>>>>,
    ) -> Result<Self, pkrs_ui::UiError> {
        let (vw, vh) = renderer.view_size();
        let margin = 8.0f32;
        let wf = vw as f32;
        let vf = vh as f32;
        let panel_h = (vf * 0.22).floor();
        let panel = Rect::new(margin, margin, wf - 2.0 * margin, panel_h);
        Ok(Self {
            session: ChoiceSession::new(options)?,
            theme,
            panel,
            result,
        })
    }
}

impl GameState for ChoiceState {
    fn id(&self) -> &str {
        "choice"
    }

    fn is_opaque(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        match self.session.poll(&ctx.input) {
            Some(ChoicePollOutcome::Picked(ix)) => {
                let a = Action::Builtin(BuiltinAction::Confirm);
                ctx.input.consume(a);
                self.result.set(Some(Some(ix)));
                Transition::Pop
            }
            Some(ChoicePollOutcome::Cancelled) => {
                let a = Action::Builtin(BuiltinAction::Cancel);
                ctx.input.consume(a);
                self.result.set(Some(None));
                Transition::Pop
            }
            None => Transition::None,
        }
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        let tint = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 170,
        };
        self.theme.skin.draw(renderer, self.panel, tint);
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        let lh = u32_px(self.theme.font.line_height) * scale_f(style.scale);
        let base_x = self.panel.x + 28.0;
        let base_y = self.panel.y + 12.0;
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
    }
}
