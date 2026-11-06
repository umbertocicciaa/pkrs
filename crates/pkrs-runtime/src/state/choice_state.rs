//! Choice menu overlay (§4.1, §5.5).

use pkrs_platform::{Action, BuiltinAction};
use pkrs_render::BitmapFont;
use pkrs_render::{Color, Layer, Rect, TextStyle};
use pkrs_ui::{ChoiceOption, ChoiceSession, WindowSkin};

use super::{GameState, Transition};
use crate::ctx::RuntimeCtx;

const FONT_RON: &str = include_str!("../../../pkrs-render/tests/fixtures/minimal.ron");

/// Non-opaque choice list over the scene.
pub struct ChoiceState {
    session: ChoiceSession,
    skin: WindowSkin,
    panel: Rect,
    font: BitmapFont,
    font_tex: pkrs_render::TextureHandle,
}

impl ChoiceState {
    /// Builds skin and font resources on `renderer`.
    pub fn new(
        renderer: &mut pkrs_render::Renderer,
        options: Vec<ChoiceOption>,
    ) -> Result<Self, pkrs_ui::UiError> {
        let tw = 32u32;
        let th = 32u32;
        let skin_px: Vec<u8> =
            std::iter::repeat_n([200u8, 200u8, 220u8, 255u8], (tw * th) as usize)
                .flatten()
                .collect();
        let tex = renderer.create_texture(tw, th, &skin_px)?;
        let skin = WindowSkin::new(tex, tw, th, pkrs_ui::NineSliceSpec::default())?;
        let font: BitmapFont = pkrs_render::parse_glyph_map_ron(FONT_RON)?;
        let fw = font.atlas_width;
        let fh = font.atlas_height;
        let font_tex = renderer.create_texture(fw, fh, &vec![0u8; (fw * fh * 4) as usize])?;
        let (vw, vh) = renderer.view_size();
        let margin = 8.0;
        let panel = Rect::new(margin, margin, vw as f32 - 2.0 * margin, vh as f32 * 0.22);
        Ok(Self {
            session: ChoiceSession::new(options),
            skin,
            panel,
            font,
            font_tex,
        })
    }
}

impl GameState for ChoiceState {
    fn id(&self) -> &'static str {
        "choice"
    }

    fn is_opaque(&self) -> bool {
        false
    }

    fn update(&mut self, ctx: &mut RuntimeCtx) -> Transition {
        if let Some(_ix) = self.session.poll(&ctx.input) {
            let a = Action::Builtin(BuiltinAction::Confirm);
            ctx.input.consume(a);
            Transition::Pop
        } else {
            Transition::None
        }
    }

    fn render(&self, renderer: &mut pkrs_render::Renderer, _alpha: f32) {
        let tint = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 170,
        };
        self.skin.draw(renderer, self.panel, tint);
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        let lh = pkrs_render::text::util::u32_px(self.font.line_height)
            * pkrs_render::text::util::scale_f(style.scale);
        let base_x = self.panel.x + 28.0;
        let base_y = self.panel.y + 12.0;
        for (i, o) in self.session.options().iter().enumerate() {
            let y = base_y + lh * i as f32;
            let prefix = if self.session.cursor() == i {
                "> "
            } else {
                "  "
            };
            renderer.draw_text(&self.font, self.font_tex, prefix, base_x - 18.0, y, style);
            renderer.draw_text(&self.font, self.font_tex, &o.label, base_x, y, style);
        }
    }
}
