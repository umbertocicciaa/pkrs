//! Script dialogue boxes over the overworld (`ui.say`, `ui.choose`, `ui.yes_no`, §5.5).
//!
//! A [`ScriptOverlay`] shows one [`UiRequest`]: a typewriter text box closed with Confirm, or
//! a choice list (Up/Down, Confirm picks, Cancel backs out). When it closes it hands back the
//! answer the waiting script task resumes with: `nil` for text, the 1-based index (or `nil`)
//! for choices.

use pkrs_platform::{Action, BuiltinAction, Input};
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_script::ScriptValue;
use pkrs_ui::{
    ChoiceOption, ChoicePollOutcome, ChoiceSession, DialogueAdvance, DialogueSession,
    colored_line_char_count, draw_colored_lines, draw_more_arrow,
};

use super::world::UiRequest;
use crate::ui_theme::UiTheme;

/// Glyphs revealed per tick by script dialogue.
pub const CHARS_PER_TICK: u32 = 2;

enum Body {
    Say(DialogueSession),
    Choose {
        prompt: Option<String>,
        session: ChoiceSession,
    },
}

/// One open script dialogue.
pub struct ScriptOverlay {
    request: UiRequest,
    body: Body,
    panel: Rect,
    choice_panel: Rect,
}

fn panels(view: (u32, u32)) -> (Rect, Rect) {
    let (vw, vh) = (view.0 as f32, view.1 as f32);
    let margin = 8.0f32;
    let panel_h = (vh * 0.28).floor();
    let text = Rect::new(margin, vh - panel_h - margin, vw - 2.0 * margin, panel_h);
    let choice_w = (vw * 0.4).floor();
    let choice = Rect::new(vw - choice_w - margin, margin, choice_w, (vh * 0.4).floor());
    (text, choice)
}

impl ScriptOverlay {
    /// Opens `request` laid out for a `view`-sized screen.
    pub fn open(request: UiRequest, theme: &UiTheme, view: (u32, u32)) -> Self {
        let (panel, choice_panel) = panels(view);
        let body = match &request {
            UiRequest::Say(text) => Body::Say(DialogueSession::new(
                text,
                &theme.font,
                panel.width - 24.0,
                panel.height - 20.0,
                1,
                CHARS_PER_TICK,
                &mut |_| String::new(),
            )),
            UiRequest::Choose { prompt, options } => {
                let options: Vec<ChoiceOption> = options
                    .iter()
                    .map(|label| ChoiceOption {
                        label: label.clone(),
                    })
                    .collect();
                match ChoiceSession::new(options) {
                    Ok(session) => Body::Choose {
                        prompt: prompt.clone(),
                        session,
                    },
                    // The bindings never send an empty list; show a closable box instead.
                    Err(_) => Body::Say(DialogueSession::new(
                        prompt.as_deref().unwrap_or(""),
                        &theme.font,
                        panel.width - 24.0,
                        panel.height - 20.0,
                        1,
                        CHARS_PER_TICK,
                        &mut |_| String::new(),
                    )),
                }
            }
        };
        Self {
            request,
            body,
            panel,
            choice_panel,
        }
    }

    /// What is shown.
    #[must_use]
    pub fn request(&self) -> &UiRequest {
        &self.request
    }

    /// Highlighted option (0-based) of a choice list.
    #[must_use]
    pub fn cursor(&self) -> Option<usize> {
        match &self.body {
            Body::Choose { session, .. } => Some(session.cursor()),
            Body::Say(_) => None,
        }
    }

    /// One tick of input. Returns the answer once the box closes.
    pub fn update(&mut self, input: &mut Input) -> Option<ScriptValue> {
        match &mut self.body {
            Body::Say(session) => {
                let confirm = Action::Builtin(BuiltinAction::Confirm);
                let pressed = input.triggered(confirm);
                if pressed {
                    input.consume(confirm);
                }
                match session.tick(pressed) {
                    DialogueAdvance::Continue => None,
                    DialogueAdvance::Done => Some(ScriptValue::Nil),
                }
            }
            Body::Choose { session, .. } => match session.poll(input)? {
                ChoicePollOutcome::Picked(i) => {
                    input.consume(Action::Builtin(BuiltinAction::Confirm));
                    Some(ScriptValue::Int(i64::try_from(i + 1).unwrap_or(i64::MAX)))
                }
                ChoicePollOutcome::Cancelled => {
                    input.consume(Action::Builtin(BuiltinAction::Cancel));
                    Some(ScriptValue::Nil)
                }
            },
        }
    }

    /// Draws the box (between `begin_frame` and `end_frame`).
    pub fn render(&self, renderer: &mut Renderer, theme: &UiTheme) {
        let tint = Color {
            r: 255,
            g: 255,
            b: 255,
            a: 180,
        };
        let style = TextStyle {
            tint: Color::WHITE,
            scale: 1,
            layer: Layer::Ui,
        };
        match &self.body {
            Body::Say(session) => {
                theme.skin.draw(renderer, self.panel, tint);
                let vis = session.visible_glyphs();
                draw_colored_lines(
                    renderer,
                    &theme.font,
                    theme.font_tex,
                    session.lines(),
                    vis,
                    self.panel.x + 12.0,
                    self.panel.y + 10.0,
                    1,
                    Layer::Ui,
                );
                if session.waiting_for_confirm && vis >= colored_line_char_count(session.lines()) {
                    let ax = self.panel.x + self.panel.width - 24.0;
                    let ay = self.panel.y + self.panel.height - 18.0;
                    draw_more_arrow(renderer, theme.font_tex, ax, ay, Color::WHITE);
                }
            }
            Body::Choose { prompt, session } => {
                if let Some(prompt) = prompt {
                    theme.skin.draw(renderer, self.panel, tint);
                    renderer.draw_text(
                        &theme.font,
                        theme.font_tex,
                        prompt,
                        self.panel.x + 12.0,
                        self.panel.y + 10.0,
                        style,
                    );
                }
                theme.skin.draw(renderer, self.choice_panel, tint);
                let lh = theme.font.line_height as f32;
                for (i, option) in session.options().iter().enumerate() {
                    let y = self.choice_panel.y + 10.0 + lh * i as f32;
                    let prefix = if session.cursor() == i { "> " } else { "  " };
                    renderer.draw_text(
                        &theme.font,
                        theme.font_tex,
                        &format!("{prefix}{}", option.label),
                        self.choice_panel.x + 10.0,
                        y,
                        style,
                    );
                }
            }
        }
    }
}

/// A full-screen fade driven by `camera.fade_out` / `camera.fade_in`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenFade {
    from: f32,
    to: f32,
    ticks: u32,
    elapsed: u32,
}

impl ScreenFade {
    /// Current darkness, `0.0` (clear) to `1.0` (black).
    #[must_use]
    pub fn level(&self) -> f32 {
        if self.ticks == 0 || self.elapsed >= self.ticks {
            return self.to;
        }
        let t = self.elapsed as f32 / self.ticks as f32;
        self.from + (self.to - self.from) * t
    }

    /// Starts fading to `to` over `ticks` ticks from the current level.
    pub fn start(&mut self, to: f32, ticks: u32) {
        self.from = self.level();
        self.to = to.clamp(0.0, 1.0);
        self.ticks = ticks;
        self.elapsed = 0;
    }

    /// One tick.
    pub fn tick(&mut self) {
        self.elapsed = self.elapsed.saturating_add(1).min(self.ticks);
    }

    /// Draws the black overlay over the whole view.
    pub fn render(&self, renderer: &mut Renderer) {
        let level = self.level();
        if level <= 0.0 {
            return;
        }
        let (w, h) = renderer.view_size();
        let color = Color {
            r: 0,
            g: 0,
            b: 0,
            a: (level * 255.0).round() as u8,
        };
        renderer.draw_filled_rect(Rect::new(0.0, 0.0, w as f32, h as f32), color, Layer::Ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fade_interpolates_and_holds() {
        let mut fade = ScreenFade::default();
        assert_eq!(fade.level(), 0.0);
        fade.start(1.0, 4);
        fade.tick();
        fade.tick();
        assert!((fade.level() - 0.5).abs() < 1e-6);
        fade.tick();
        fade.tick();
        fade.tick();
        assert_eq!(fade.level(), 1.0);
        fade.start(0.0, 0);
        assert_eq!(fade.level(), 0.0);
    }
}
