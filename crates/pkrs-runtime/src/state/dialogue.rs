//! Dialogue overlay: typewriter + 9-slice skin (§4.1, §5.5).

use pkrs_platform::{Action, BuiltinAction};
use pkrs_render::BitmapFont;
use pkrs_render::{Color, Layer, Rect};
use pkrs_ui::{
    DialogueAdvance, DialogueSession, WindowSkin, colored_line_char_count, draw_colored_lines,
    draw_more_arrow,
};

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;

const FONT_RON: &str = include_str!("../../../pkrs-render/tests/fixtures/minimal.ron");

fn skin_atlas_rgba() -> Vec<u8> {
    let w = 32u32;
    let h = 32u32;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            let c = if (x + y) % 2 == 0 { 40u8 } else { 25u8 };
            rgba[i] = c;
            rgba[i + 1] = c.saturating_add(30);
            rgba[i + 2] = c.saturating_add(50);
            rgba[i + 3] = 255;
        }
    }
    rgba
}

/// Non-opaque dialogue box over the scene (§4.1).
pub struct DialogueState {
    session: DialogueSession,
    skin: WindowSkin,
    font: BitmapFont,
    font_tex: pkrs_render::TextureHandle,
    text_scale: u32,
    panel: Rect,
}

impl DialogueState {
    /// Builds fonts and skin on `renderer` (fixtures/minimal.ron + blank atlas).
    pub fn new(
        renderer: &mut pkrs_render::Renderer,
        script: &str,
        chars_per_tick: u32,
    ) -> Result<Self, pkrs_ui::UiError> {
        let skin_px = skin_atlas_rgba();
        let tw = 32u32;
        let th = 32u32;
        let tex_skin = renderer.create_texture(tw, th, &skin_px)?;
        let skin = WindowSkin::new(tex_skin, tw, th, pkrs_ui::NineSliceSpec::default())?;
        let font: BitmapFont = pkrs_render::parse_glyph_map_ron(FONT_RON)?;
        let fw = font.atlas_width;
        let fh = font.atlas_height;
        let font_tex = renderer.create_texture(fw, fh, &vec![0u8; (fw * fh * 4) as usize])?;
        let (vw, vh) = renderer.view_size();
        let margin = 8.0;
        let panel_h = vh as f32 * 0.28;
        let panel = Rect::new(
            margin,
            vh as f32 - panel_h - margin,
            vw as f32 - 2.0 * margin,
            panel_h,
        );
        let text_width = panel.width - 24.0;
        let scale = 1u32;
        let session =
            DialogueSession::new(script, &font, text_width, scale, chars_per_tick, &|_| {
                String::new()
            });
        Ok(Self {
            session,
            skin,
            font,
            font_tex,
            text_scale: scale,
            panel,
        })
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

    fn render(&self, renderer: &mut pkrs_render::Renderer, _alpha: f32) {
        let tint_panel = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 180,
        };
        self.skin.draw(renderer, self.panel, tint_panel);
        let tx = self.panel.x + 12.0;
        let ty = self.panel.y + 10.0;
        let vis = self.session.visible_glyphs();
        draw_colored_lines(
            renderer,
            &self.font,
            self.font_tex,
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
            draw_more_arrow(renderer, self.font_tex, ax, ay, Color::WHITE);
        }
    }
}
