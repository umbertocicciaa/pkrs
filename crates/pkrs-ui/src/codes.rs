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

/// Expands `{player}`, `{var:name}`, strips `{color:…}` / `{pause:N}` into timing metadata only.
pub fn expand_dialogue<F>(raw: &str, mut lookup_var: F) -> Vec<DialogueSegment>
where
    F: FnMut(&str) -> String,
{
    let tokens = parse_control_tokens(raw);
    let mut out: Vec<DialogueSegment> = Vec::new();
    let mut cur_tint = Color::WHITE;
    let mut cur = String::new();

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
            ControlToken::Text(s) => cur.push_str(&s),
            ControlToken::Color(name) => {
                flush(&mut cur, &mut out, cur_tint);
                cur_tint = color_from_name(&name);
            }
            ControlToken::Pause(_) | ControlToken::PageBreak => {}
            ControlToken::PlayerName => cur.push_str("Player"),
            ControlToken::Var(name) => cur.push_str(&lookup_var(&name)),
        }
    }
    flush(&mut cur, &mut out, cur_tint);
    if out.is_empty() {
        out.push(DialogueSegment {
            text: String::new(),
            tint: Color::WHITE,
        });
    }
    out
}

/// Collects `{pause:N}` durations in order; same index as reveal position in the flattened string.
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
enum ControlToken {
    Text(String),
    Color(String),
    Pause(u32),
    PageBreak,
    PlayerName,
    Var(String),
}

fn parse_control_tokens(raw: &str) -> Vec<ControlToken> {
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
    if let Some(rest) = inner.strip_prefix("pause:") {
        if let Ok(n) = rest.trim().parse::<u32>() {
            return ControlToken::Pause(n);
        }
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
        let segs = expand_dialogue("Hi {player}, score {var:pts}.", |k| {
            if k == "pts" {
                "10".into()
            } else {
                String::new()
            }
        });
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "Hi Player, score 10.");
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
    fn pause_positions_match_char_index() {
        let p = pause_schedule("ab{pause:5}c");
        assert_eq!(p, vec![(2, 5)]);
    }
}
