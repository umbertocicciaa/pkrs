//! Dialogue overlay: typewriter + 9-slice skin (§4.1, §5.5).

use pkrs_platform::{Action, BuiltinAction};
use pkrs_render::{Color, Layer, Rect, Renderer};
use pkrs_ui::{
    DialogueAdvance, DialogueSession, colored_line_char_count, draw_colored_lines, draw_more_arrow,
};

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;
use crate::ui_theme::UiTheme;

/// Non-opaque dialogue box over the scene (§4.1).
pub struct DialogueState {
    session: DialogueSession,
    theme: UiTheme,
    text_scale: u32,
    panel: Rect,
}

impl DialogueState {
    /// Uses pre-uploaded `theme` (font + skin); `vars` resolves `{var:…}` and `player`.
    pub fn new(
        theme: UiTheme,
        renderer: &Renderer,
        script: &str,
        chars_per_tick: u32,
        mut vars: impl FnMut(&str) -> String,
    ) -> Self {
        let (vw, vh) = renderer.view_size();
        let margin = 8.0f32;
        let vf = vh as f32;
        let wf = vw as f32;
        let panel_h = (vf * 0.28).floor();
        let panel = Rect::new(margin, vf - panel_h - margin, wf - 2.0 * margin, panel_h);
        let text_width = panel.width - 24.0;
        let text_height = panel.height - 20.0;
        let scale = 1u32;
        let session = DialogueSession::new(
            script,
            &theme.font,
            text_width,
            text_height,
            scale,
            chars_per_tick,
            &mut vars,
        );
        Self {
            session,
            theme,
            text_scale: scale,
            panel,
        }
    }

    fn take_confirm(ctx: &mut RuntimeCtx) -> bool {
        let a = Action::Builtin(BuiltinAction::Confirm);
        if ctx.input.triggered(a) {
            ctx.input.consume(a);
            true
        } else {
            false
        }
    }
}

impl GameState for DialogueState {
    fn id(&self) -> &'static str {
        "dialogue"
    }

    fn is_opaque(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        let confirm = Self::take_confirm(ctx);
        match self.session.tick(confirm) {
            DialogueAdvance::Continue => Transition::None,
            DialogueAdvance::Done => Transition::Pop,
        }
    }

    fn render(&self, renderer: &mut Renderer, _alpha: f32) {
        let tint_panel = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 180,
        };
        self.theme.skin.draw(renderer, self.panel, tint_panel);
        let tx = self.panel.x + 12.0;
        let ty = self.panel.y + 10.0;
        let vis = self.session.visible_glyphs();
        draw_colored_lines(
            renderer,
            &self.theme.font,
            self.theme.font_tex,
            self.session.lines(),
            vis,
            tx,
            ty,
            self.text_scale,
            Layer::Ui,
        );
        let cap = colored_line_char_count(self.session.lines());
        if self.session.waiting_for_confirm && vis >= cap {
            let ax = self.panel.x + self.panel.width - 24.0;
            let ay = self.panel.y + self.panel.height - 18.0;
            draw_more_arrow(renderer, self.theme.font_tex, ax, ay, Color::WHITE);
        }
    }
}
