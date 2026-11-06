//! Typewriter dialogue over coloured, wrapped text (§5.5).

use pkrs_render::BitmapFont;
use pkrs_render::Color;

use crate::codes::{ControlToken, dialogue_page_token_slices, expand_page_tokens};
use crate::layout::{colored_line_char_count, flatten_segments, wrap_colored_lines};

/// One page of dialogue after control-code expansion and line splitting.
#[derive(Clone, Debug)]
pub struct DialoguePage {
    /// Wrapped lines for layout/drawing (at most `max_lines` rows).
    pub lines: Vec<Vec<(char, Color)>>,
    /// Glyphs in `lines` (including spaces).
    pub glyph_count: usize,
    /// Pauses `(glyph_index_in_this_page, ticks)` — triggers when `visible` reaches `glyph_index`.
    pub page_pauses: Vec<(usize, u32)>,
}

impl DialoguePage {
    fn build(
        raw_tokens: &[ControlToken],
        font: &BitmapFont,
        text_width: f32,
        max_lines: usize,
        scale: u32,
        vars: &mut dyn FnMut(&str) -> String,
    ) -> Vec<Self> {
        let (segments, pauses_flat) = expand_page_tokens(raw_tokens, vars);
        let flat = flatten_segments(&segments);
        let lines_full = wrap_colored_lines(font, &flat, text_width, scale);
        let max_lines = max_lines.max(1);

        let full_wrapped: Vec<(usize, u32)> = pauses_flat
            .iter()
            .map(|&(fp, ticks)| {
                let end = fp.min(flat.len());
                let prefix = &flat[..end];
                let wrapped = wrap_colored_lines(font, prefix, text_width, scale);
                let wpos = colored_line_char_count(&wrapped);
                (wpos, ticks)
            })
            .collect();

        let mut pages = Vec::new();
        let mut line_start = 0usize;
        while line_start < lines_full.len() {
            let line_end = (line_start + max_lines).min(lines_full.len());
            let chunk: Vec<Vec<(char, Color)>> = lines_full[line_start..line_end].to_vec();
            let chunk_glyphs: usize = chunk.iter().map(|r| r.len()).sum();
            let cum_before: usize = lines_full[..line_start].iter().map(|r| r.len()).sum();

            let mut page_pauses: Vec<(usize, u32)> = full_wrapped
                .iter()
                .filter_map(|&(wpos, ticks)| {
                    if wpos > cum_before && wpos <= cum_before + chunk_glyphs {
                        Some((wpos - cum_before, ticks))
                    } else {
                        None
                    }
                })
                .collect();
            page_pauses.sort_by_key(|(pos, _)| *pos);

            let glyph_count = colored_line_char_count(&chunk);
            pages.push(DialoguePage {
                lines: chunk,
                glyph_count,
                page_pauses,
            });
            line_start = line_end;
        }
        if pages.is_empty() {
            pages.push(DialoguePage {
                lines: vec![Vec::new()],
                glyph_count: 0,
                page_pauses: Vec::new(),
            });
        }
        pages
    }
}

/// In-progress dialogue (typewriter, `{page}`, `{pause:N}`).
#[derive(Clone, Debug)]
pub struct DialogueSession {
    pages: Vec<DialoguePage>,
    page_index: usize,
    visible: usize,
    /// Glyphs revealed per tick when not paused (`≥ 1`).
    pub chars_per_tick: u32,
    pause_ticks: u32,
    /// Waiting for player confirm (end of page or `{pause:0}`).
    pub waiting_for_confirm: bool,
    /// `tick` returned [`DialogueAdvance::Done`]; [`lines`](Self::lines) stay on the last page.
    pub finished: bool,
    pause_cursor: usize,
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
    /// Parses `{page}` breaks from token stream. `vars` resolves `{var:name}` and `player`.
    pub fn new(
        script: &str,
        font: &BitmapFont,
        text_width: f32,
        text_height: f32,
        scale: u32,
        chars_per_tick: u32,
        vars: &mut dyn FnMut(&str) -> String,
    ) -> Self {
        let lh = font.line_height as f32 * scale_f(scale);
        let max_lines = if lh > 0.0 && text_height > 0.0 {
            ((text_height / lh).floor() as usize).max(1)
        } else {
            usize::MAX
        };

        let page_slices = dialogue_page_token_slices(script);
        let mut pages = Vec::new();
        for slice in page_slices {
            let trimmed: Vec<ControlToken> = slice
                .into_iter()
                .filter(
                    |t| !matches!(t, ControlToken::Text(s) if s.chars().all(|c| c.is_whitespace())),
                )
                .collect();
            if trimmed.is_empty() && pages.is_empty() {
                continue;
            }
            pages.extend(DialoguePage::build(
                &trimmed, font, text_width, max_lines, scale, vars,
            ));
        }
        if pages.is_empty() {
            pages.extend(DialoguePage::build(
                &[],
                font,
                text_width,
                max_lines,
                scale,
                vars,
            ));
        }

        Self {
            pages,
            page_index: 0,
            visible: 0,
            chars_per_tick: chars_per_tick.max(1),
            pause_ticks: 0,
            waiting_for_confirm: false,
            finished: false,
            pause_cursor: 0,
        }
    }

    fn page(&self) -> Option<&DialoguePage> {
        self.pages.get(self.page_index)
    }

    /// Current wrapped lines (last page when [`finished`](Self::finished)).
    pub fn lines(&self) -> &[Vec<(char, Color)>] {
        self.page().map(|p| p.lines.as_slice()).unwrap_or(&[])
    }

    /// Visible glyphs on the current page.
    pub fn visible_glyphs(&self) -> usize {
        let cap = self.page().map(|p| p.glyph_count).unwrap_or(0);
        self.visible.min(cap)
    }

    /// Zero-based index of the page currently shown.
    #[must_use]
    pub fn page_index(&self) -> usize {
        self.page_index
    }

    /// Total glyphs on the current page.
    #[must_use]
    pub fn current_page_glyph_count(&self) -> usize {
        self.page().map(|p| p.glyph_count).unwrap_or(0)
    }

    fn advance_page(&mut self) -> DialogueAdvance {
        if self.page_index + 1 >= self.pages.len() {
            self.finished = true;
            return DialogueAdvance::Done;
        }
        self.page_index += 1;
        self.visible = 0;
        self.pause_ticks = 0;
        self.waiting_for_confirm = false;
        self.pause_cursor = 0;
        DialogueAdvance::Continue
    }

    /// One tick: `confirm` is the primary action edge (advance page / skip typewriter / break wait).
    pub fn tick(&mut self, confirm: bool) -> DialogueAdvance {
        if self.finished {
            return DialogueAdvance::Done;
        }
        let Some(page) = self.pages.get(self.page_index) else {
            self.finished = true;
            return DialogueAdvance::Done;
        };
        let cap = page.glyph_count;

        if confirm && !self.waiting_for_confirm && self.visible < cap {
            self.visible = cap;
            self.pause_ticks = 0;
            self.waiting_for_confirm = true;
            return DialogueAdvance::Continue;
        }

        if self.pause_ticks > 0 {
            if confirm {
                self.visible = cap;
                self.pause_ticks = 0;
                self.waiting_for_confirm = true;
                return DialogueAdvance::Continue;
            }
            self.pause_ticks = self.pause_ticks.saturating_sub(1);
            return DialogueAdvance::Continue;
        }

        if self.waiting_for_confirm {
            if confirm {
                self.waiting_for_confirm = false;
                if self.visible >= cap {
                    return self.advance_page();
                }
            }
            return DialogueAdvance::Continue;
        }

        if self.visible >= cap {
            self.waiting_for_confirm = true;
            return DialogueAdvance::Continue;
        }

        let target = (self.visible + self.chars_per_tick as usize).min(cap);

        while self.pause_cursor < page.page_pauses.len() {
            let (pos, ticks) = page.page_pauses[self.pause_cursor];
            if pos > self.visible && pos <= target {
                self.visible = pos;
                self.pause_cursor += 1;
                if ticks > 0 {
                    self.pause_ticks = ticks;
                } else {
                    self.waiting_for_confirm = true;
                }
                return DialogueAdvance::Continue;
            }
            if pos <= self.visible {
                self.pause_cursor += 1;
                continue;
            }
            break;
        }

        self.visible = target;

        if self.pause_cursor < page.page_pauses.len() {
            let (pos, ticks) = page.page_pauses[self.pause_cursor];
            if pos == self.visible {
                self.pause_cursor += 1;
                if ticks > 0 {
                    self.pause_ticks = ticks;
                } else {
                    self.waiting_for_confirm = true;
                }
                return DialogueAdvance::Continue;
            }
        }

        if self.visible >= cap {
            self.waiting_for_confirm = true;
        }

        DialogueAdvance::Continue
    }
}

#[inline]
fn scale_f(scale: u32) -> f32 {
    scale.max(1) as f32
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
        let mut s = DialogueSession::new("Hello", &f, 120.0, 80.0, 1, 2, &mut |_| String::new());
        let _ = s.tick(false);
        assert_eq!(s.visible_glyphs(), 2);
        let _ = s.tick(false);
        assert_eq!(s.visible_glyphs(), 4);
    }

    #[test]
    fn confirm_advances_second_page() {
        let f = font();
        let mut s =
            DialogueSession::new("AB{page}CD", &f, 200.0, 80.0, 1, 10, &mut |_| String::new());
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

    #[test]
    fn confirm_skips_typewriter_mid_page() {
        let f = font();
        let mut s =
            DialogueSession::new("AAAAAAAAAA", &f, 200.0, 80.0, 1, 1, &mut |_| String::new());
        let _ = s.tick(false);
        assert_eq!(s.visible_glyphs(), 1);
        let _ = s.tick(true);
        assert_eq!(s.visible_glyphs(), 10);
        assert!(s.waiting_for_confirm);
    }

    #[test]
    fn lines_available_after_done() {
        let f = font();
        let mut s = DialogueSession::new("OK", &f, 100.0, 80.0, 1, 100, &mut |_| String::new());
        let _ = s.tick(false);
        assert!(s.waiting_for_confirm);
        let _ = s.tick(true);
        assert_eq!(s.tick(false), DialogueAdvance::Done);
        assert!(s.finished);
        assert!(!s.lines().is_empty());
        let _ = s.visible_glyphs();
    }

    #[test]
    fn pause_triggers_after_expanded_text() {
        let f = font();
        let mut lookup = |k: &str| -> String {
            if k == "x" {
                "12345".into()
            } else {
                String::new()
            }
        };
        let mut s = DialogueSession::new("{var:x}B{pause:2}C", &f, 200.0, 80.0, 1, 10, &mut lookup);
        let _ = s.tick(false);
        assert_eq!(s.visible_glyphs(), 6);
        assert_eq!(s.pause_ticks, 2);
    }
}
