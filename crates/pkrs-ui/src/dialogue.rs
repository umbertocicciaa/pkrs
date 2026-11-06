//! Typewriter dialogue over coloured, wrapped text (§5.5).

use pkrs_render::BitmapFont;
use pkrs_render::Color;

use crate::codes::{expand_dialogue, pause_schedule};
use crate::layout::{colored_line_char_count, flatten_segments, wrap_colored_lines};

/// One page of dialogue after control-code expansion.
#[derive(Clone, Debug)]
pub struct DialoguePage {
    /// Wrapped lines for layout/drawing.
    pub lines: Vec<Vec<(char, Color)>>,
    /// Glyphs in `lines` (including spaces).
    pub glyph_count: usize,
}

impl DialoguePage {
    fn build(
        raw: &str,
        max_width: f32,
        scale: u32,
        font: &BitmapFont,
        vars: &dyn Fn(&str) -> String,
    ) -> Self {
        let segments = expand_dialogue(raw, |k| vars(k));
        let flat = flatten_segments(&segments);
        let lines = wrap_colored_lines(font, &flat, max_width, scale);
        let glyph_count = colored_line_char_count(&lines);
        Self { lines, glyph_count }
    }
}

/// In-progress dialogue (typewriter, `{page}`, `{pause:N}`).
#[derive(Clone, Debug)]
pub struct DialogueSession {
    full_script: String,
    pages: Vec<DialoguePage>,
    /// Pauses `(glyph_index, ticks)` after that many glyphs are visible on the **current** page.
    page_pauses: Vec<(usize, u32)>,
    page_index: usize,
    visible: usize,
    /// Glyphs revealed per 60 Hz tick when not paused.
    pub chars_per_tick: u32,
    pause_ticks: u32,
    /// Waiting for player confirm (end of page or `{pause:0}`).
    pub waiting_for_confirm: bool,
}

/// Result of [`DialogueSession::tick`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DialogueAdvance {
    /// More content remains (another page or more ticks on this page).
    Continue,
    /// Last page finished; session is complete.
    Done,
}

impl DialogueSession {
    /// Splits `script` on `{page}`; `vars` resolves `{var:name}`.
    pub fn new(
        script: &str,
        font: &BitmapFont,
        text_width: f32,
        scale: u32,
        chars_per_tick: u32,
        vars: &dyn Fn(&str) -> String,
    ) -> Self {
        let mut pages = Vec::new();
        for part in script.split("{page}") {
            let p = part.trim();
            if p.is_empty() && pages.is_empty() {
                continue;
            }
            pages.push(DialoguePage::build(p, text_width, scale, font, vars));
        }
        if pages.is_empty() {
            pages.push(DialoguePage::build("", text_width, scale, font, vars));
        }
        let first_raw = script.split("{page}").next().map(str::trim).unwrap_or("");
        let page_pauses = pause_schedule(first_raw);
        Self {
            full_script: script.to_string(),
            pages,
            page_pauses,
            page_index: 0,
            visible: 0,
            chars_per_tick: chars_per_tick.max(1),
            pause_ticks: 0,
            waiting_for_confirm: false,
        }
    }

    fn segment_raw(&self, idx: usize) -> &str {
        self.full_script
            .split("{page}")
            .nth(idx)
            .map(str::trim)
            .unwrap_or("")
    }

    fn page(&self) -> &DialoguePage {
        &self.pages[self.page_index]
    }

    /// Current wrapped lines.
    pub fn lines(&self) -> &[Vec<(char, Color)>] {
        &self.page().lines
    }

    /// Visible glyphs on the current page.
    pub fn visible_glyphs(&self) -> usize {
        self.visible.min(self.page().glyph_count)
    }

    /// Zero-based index of the page currently shown (`{page}` splits).
    #[must_use]
    pub fn page_index(&self) -> usize {
        self.page_index
    }

    /// Total glyphs on the current page.
    #[must_use]
    pub fn current_page_glyph_count(&self) -> usize {
        self.page().glyph_count
    }

    fn finished(&self) -> bool {
        self.page_index >= self.pages.len()
    }

    fn next_pause(&self) -> Option<u32> {
        self.page_pauses
            .iter()
            .find(|(pos, _)| *pos == self.visible)
            .map(|(_, ticks)| *ticks)
    }

    /// One tick: `confirm` is the primary action edge (advance page / break wait).
    pub fn tick(&mut self, confirm: bool) -> DialogueAdvance {
        if self.finished() {
            return DialogueAdvance::Done;
        }

        if self.pause_ticks > 0 {
            self.pause_ticks = self.pause_ticks.saturating_sub(1);
            return DialogueAdvance::Continue;
        }

        if self.waiting_for_confirm {
            if confirm {
                self.waiting_for_confirm = false;
                if self.visible >= self.page().glyph_count {
                    self.page_index += 1;
                    self.visible = 0;
                    if self.page_index >= self.pages.len() {
                        return DialogueAdvance::Done;
                    }
                    self.page_pauses = pause_schedule(self.segment_raw(self.page_index));
                }
            }
            return DialogueAdvance::Continue;
        }

        let cap = self.page().glyph_count;
        if self.visible >= cap {
            self.waiting_for_confirm = true;
            return DialogueAdvance::Continue;
        }

        self.visible = (self.visible + self.chars_per_tick as usize).min(cap);

        if let Some(wait) = self.next_pause() {
            if wait > 0 {
                self.pause_ticks = wait;
            } else {
                self.waiting_for_confirm = true;
            }
            return DialogueAdvance::Continue;
        }

        if self.visible >= cap {
            self.waiting_for_confirm = true;
        }

        DialogueAdvance::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_render::parse_glyph_map_ron;

    const GLYPH_RON: &str = include_str!("../../pkrs-render/tests/fixtures/minimal.ron");

    fn font() -> BitmapFont {
        parse_glyph_map_ron(GLYPH_RON).expect("font")
    }

    #[test]
    fn typewriter_advances_by_chars_per_tick() {
        let f = font();
        let mut s = DialogueSession::new("Hello", &f, 120.0, 1, 2, &|_| String::new());
        let _ = s.tick(false);
        assert_eq!(s.visible_glyphs(), 2);
        let _ = s.tick(false);
        assert_eq!(s.visible_glyphs(), 4);
    }

    #[test]
    fn confirm_advances_second_page() {
        let f = font();
        let mut s = DialogueSession::new("AB{page}CD", &f, 200.0, 1, 10, &|_| String::new());
        for _ in 0..20 {
            if s.waiting_for_confirm {
                break;
            }
            assert_eq!(s.tick(false), DialogueAdvance::Continue);
        }
        assert!(s.waiting_for_confirm);
        assert_eq!(s.tick(true), DialogueAdvance::Continue);
        assert_eq!(s.page_index(), 1);
    }
}
