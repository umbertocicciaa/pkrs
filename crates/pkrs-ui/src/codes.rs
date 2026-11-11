//! Inline control codes for dialogue (§5.5).

use pkrs_render::Color;

/// One segment of parsed dialogue with its effective colour.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DialogueSegment {
    /// Visible text (UTF-8).
    pub text: String,
    /// Tint for glyphs in this segment.
    pub tint: Color,
}

/// Expands `{player}`, `{var:name}`, strips `{color:…}` / `{pause:N}` / `{page}` into metadata only.
pub fn expand_dialogue<F>(raw: &str, mut lookup_var: F) -> Vec<DialogueSegment>
where
    F: FnMut(&str) -> String,
{
    let (segs, _) = expand_dialogue_with_pauses(raw, &mut lookup_var);
    segs
}

/// Like [`expand_dialogue`], plus `{pause:N}` positions measured in **expanded** glyph indices (same order as [`crate::layout::flatten_segments`]).
pub fn expand_dialogue_with_pauses<F>(
    raw: &str,
    lookup_var: &mut F,
) -> (Vec<DialogueSegment>, Vec<(usize, u32)>)
where
    F: FnMut(&str) -> String + ?Sized,
{
    let tokens = parse_control_tokens(raw);
    expand_tokens_with_pauses(&tokens, lookup_var)
}

/// Splits `raw` into pages at non-escaped `{page}` tokens; each chunk is expanded separately.
pub fn dialogue_page_token_slices(raw: &str) -> Vec<Vec<ControlToken>> {
    let tokens = parse_control_tokens(raw);
    let mut pages: Vec<Vec<ControlToken>> = vec![Vec::new()];
    for t in tokens {
        if matches!(t, ControlToken::PageBreak) {
            pages.push(Vec::new());
        } else if let Some(last) = pages.last_mut() {
            last.push(t);
        }
    }
    pages
}

pub(crate) fn expand_page_tokens<F>(
    tokens: &[ControlToken],
    lookup_var: &mut F,
) -> (Vec<DialogueSegment>, Vec<(usize, u32)>)
where
    F: FnMut(&str) -> String + ?Sized,
{
    expand_tokens_with_pauses(tokens, lookup_var)
}

fn expand_tokens_with_pauses<F>(
    tokens: &[ControlToken],
    lookup_var: &mut F,
) -> (Vec<DialogueSegment>, Vec<(usize, u32)>)
where
    F: FnMut(&str) -> String + ?Sized,
{
    let mut out: Vec<DialogueSegment> = Vec::new();
    let mut pauses: Vec<(usize, u32)> = Vec::new();
    let mut cur_tint = Color::WHITE;
    let mut cur = String::new();
    let mut glyph_pos = 0usize;

    let flush = |buf: &mut String, segs: &mut Vec<DialogueSegment>, tint: Color| {
        if buf.is_empty() {
            return;
        }
        segs.push(DialogueSegment {
            text: std::mem::take(buf),
            tint,
        });
    };

    for t in tokens {
        match t {
            ControlToken::Text(s) => {
                glyph_pos += s.chars().count();
                cur.push_str(s);
            }
            ControlToken::Color(name) => {
                flush(&mut cur, &mut out, cur_tint);
                cur_tint = color_from_name(name);
            }
            ControlToken::Pause(frames) => {
                flush(&mut cur, &mut out, cur_tint);
                pauses.push((glyph_pos, *frames));
            }
            ControlToken::PageBreak => {}
            ControlToken::PlayerName => {
                let s = lookup_var("player");
                glyph_pos += s.chars().count();
                cur.push_str(&s);
            }
            ControlToken::Var(name) => {
                let s = lookup_var(name);
                glyph_pos += s.chars().count();
                cur.push_str(&s);
            }
        }
    }
    flush(&mut cur, &mut out, cur_tint);
    if out.is_empty() {
        out.push(DialogueSegment {
            text: String::new(),
            tint: Color::WHITE,
        });
    }
    (out, pauses)
}

/// Legacy: collects `{pause:N}` using raw token positions (prefer [`expand_dialogue_with_pauses`]).
pub fn pause_schedule(raw: &str) -> Vec<(usize, u32)> {
    let tokens = parse_control_tokens(raw);
    let mut pos = 0usize;
    let mut pauses = Vec::new();
    for t in tokens {
        match t {
            ControlToken::Text(s) => pos += s.chars().count(),
            ControlToken::Pause(frames) => pauses.push((pos, frames)),
            ControlToken::PageBreak => pauses.push((pos, 0)),
            ControlToken::Color(_) | ControlToken::PlayerName | ControlToken::Var(_) => {}
        }
    }
    pauses
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ControlToken {
    Text(String),
    Color(String),
    Pause(u32),
    PageBreak,
    PlayerName,
    Var(String),
}

pub(crate) fn parse_control_tokens(raw: &str) -> Vec<ControlToken> {
    let mut out = Vec::new();
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == '{' {
            if i + 1 < chars.len() && chars[i + 1] == '{' {
                push_text(&mut out, "{");
                i += 2;
                continue;
            }
            let start = i + 1;
            let mut j = start;
            while j < chars.len() && chars[j] != '}' {
                j += 1;
            }
            if j >= chars.len() {
                push_text(&mut out, &chars[i..].iter().collect::<String>());
                break;
            }
            let inner: String = chars[start..j].iter().collect();
            i = j + 1;
            out.push(parse_inner(&inner));
            continue;
        }
        let ch = chars[i];
        push_text(&mut out, &ch.to_string());
        i += 1;
    }
    out
}

fn push_text(out: &mut Vec<ControlToken>, s: &str) {
    if s.is_empty() {
        return;
    }
    match out.last_mut() {
        Some(ControlToken::Text(prev)) => prev.push_str(s),
        _ => out.push(ControlToken::Text(s.to_string())),
    }
}

fn parse_inner(inner: &str) -> ControlToken {
    if inner == "player" {
        return ControlToken::PlayerName;
    }
    if inner == "page" {
        return ControlToken::PageBreak;
    }
    if let Some(rest) = inner.strip_prefix("color:") {
        return ControlToken::Color(rest.trim().to_string());
    }
    if let Some(rest) = inner.strip_prefix("pause:")
        && let Ok(n) = rest.trim().parse::<u32>()
    {
        return ControlToken::Pause(n);
    }
    if let Some(rest) = inner.strip_prefix("var:") {
        return ControlToken::Var(rest.trim().to_string());
    }
    ControlToken::Text(format!("{{{inner}}}"))
}

fn color_from_name(name: &str) -> Color {
    match name {
        "red" => Color {
            r: 220,
            g: 60,
            b: 60,
            a: 255,
        },
        "green" => Color {
            r: 60,
            g: 200,
            b: 80,
            a: 255,
        },
        "blue" => Color {
            r: 80,
            g: 120,
            b: 220,
            a: 255,
        },
        _ => Color::WHITE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_player_and_var() {
        let segs = expand_dialogue("Hi {player}, score {var:pts}.", |k| match k {
            "player" => "Player".into(),
            "pts" => "10".into(),
            _ => String::new(),
        });
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "Hi Player, score 10.");
    }

    #[test]
    fn player_name_via_lookup_only() {
        let segs = expand_dialogue("Hello {player}!", |k| {
            if k == "player" {
                "Pat".into()
            } else {
                String::new()
            }
        });
        assert_eq!(segs[0].text, "Hello Pat!");
    }

    #[test]
    fn color_splits_segments() {
        let segs = expand_dialogue("a{color:red}b{color:green}c", |_| String::new());
        assert_eq!(segs.len(), 3);
        assert_eq!(segs[0].text, "a");
        assert_eq!(segs[1].text, "b");
        assert_eq!(segs[2].text, "c");
    }

    #[test]
    fn pause_positions_use_expanded_text() {
        let mut lookup = |_k: &str| "ZZ".to_string();
        let (_, p) = expand_dialogue_with_pauses("{var:x}B{pause:3}C", &mut lookup);
        assert_eq!(p, vec![(3, 3)]);
    }
}
