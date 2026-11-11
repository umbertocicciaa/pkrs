//! Static facts about a Lua file, read without running it (§2.3, §6.5): global function
//! definitions and their bodies, `events.on("tick", …)` listeners, string literals
//! (`"text:…"` keys, ids registered with `data.register_*`) and whether a function sets a
//! flag, variable or map variable.
//!
//! A small tokenizer (names, strings, comments, long brackets) is enough for these patterns;
//! syntax errors are found separately by compiling the file
//! ([`check_lua_syntax`](crate::check_lua_syntax)).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;

/// Calls that record story state (§6.5): `game.set_flag`, `game.set_var`, `map.set_var`.
pub const STATE_SETTERS: &[(&str, &str)] = &[
    ("game", "set_flag"),
    ("game", "set_var"),
    ("map", "set_var"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Name(String),
    Str(String),
    Punct(char),
    Other,
}

#[derive(Clone, Debug)]
struct Token {
    tok: Tok,
    range: Range<usize>,
}

impl Token {
    fn is_name(&self, name: &str) -> bool {
        matches!(&self.tok, Tok::Name(n) if n == name)
    }

    fn is_punct(&self, c: char) -> bool {
        self.tok == Tok::Punct(c)
    }
}

/// A global function defined in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LuaFunction {
    /// Byte range of the function's name.
    pub name: Range<usize>,
    /// Token range of the definition (from `function` to its `end`).
    body: Range<usize>,
}

/// What [`LuaFacts::scan`] found in one file.
#[derive(Clone, Debug, Default)]
pub struct LuaFacts {
    tokens: Vec<Token>,
    /// Global functions (`function name(…)`, `name = function(…)`), by name.
    pub functions: BTreeMap<String, LuaFunction>,
    /// Byte ranges of the `"tick"` argument of every `events.on("tick", …)`.
    pub tick_listeners: Vec<Range<usize>>,
    /// Every string literal and its byte range (quotes included).
    pub strings: Vec<(String, Range<usize>)>,
    /// The file calls `data.register_*` (§9.4).
    pub registers_content: bool,
}

impl LuaFacts {
    /// Scans `source`.
    #[must_use]
    pub fn scan(source: &str) -> Self {
        let tokens = tokenize(source);
        let mut facts = Self::default();
        for (i, t) in tokens.iter().enumerate() {
            if let Tok::Str(value) = &t.tok {
                facts.strings.push((value.clone(), t.range.clone()));
            }
            let at = |k: usize| tokens.get(i + k);
            // events.on("tick"
            if t.is_name("events")
                && at(1).is_some_and(|x| x.is_punct('.'))
                && at(2).is_some_and(|x| x.is_name("on"))
                && at(3).is_some_and(|x| x.is_punct('('))
                && let Some(Token {
                    tok: Tok::Str(name),
                    range,
                }) = at(4)
                && name == "tick"
            {
                facts.tick_listeners.push(range.clone());
            }
            if t.is_name("data")
                && at(1).is_some_and(|x| x.is_punct('.'))
                && let Some(Token {
                    tok: Tok::Name(n), ..
                }) = at(2)
                && n.starts_with("register_")
            {
                facts.registers_content = true;
            }
            let prev = i.checked_sub(1).and_then(|p| tokens.get(p));
            // function name(
            if t.is_name("function")
                && !prev.is_some_and(|p| p.is_name("local"))
                && let (
                    Some(Token {
                        tok: Tok::Name(name),
                        range,
                    }),
                    Some(open),
                ) = (at(1), at(2))
                && open.is_punct('(')
            {
                let body = i..block_end(&tokens, i);
                facts.functions.insert(
                    name.clone(),
                    LuaFunction {
                        name: range.clone(),
                        body,
                    },
                );
            }
            // name = function(
            if let Tok::Name(name) = &t.tok {
                let assigned = at(1).is_some_and(|x| x.is_punct('='))
                    && !at(2).is_some_and(|x| x.is_punct('='))
                    && at(2).is_some_and(|x| x.is_name("function"));
                let statement_start = !prev.is_some_and(|p| {
                    p.is_punct('.')
                        || p.is_punct(':')
                        || p.is_punct(',')
                        || p.is_name("local")
                        || p.is_punct('=')
                });
                if assigned && statement_start && !is_keyword(name) {
                    let body = i + 2..block_end(&tokens, i + 2);
                    facts.functions.insert(
                        name.clone(),
                        LuaFunction {
                            name: t.range.clone(),
                            body,
                        },
                    );
                }
            }
        }
        facts.tokens = tokens;
        facts
    }

    /// `true` when `function` (a global function of this file) calls one of the
    /// [`STATE_SETTERS`], directly or through other global functions of this file.
    #[must_use]
    pub fn sets_state(&self, function: &str) -> bool {
        let mut seen = BTreeSet::new();
        self.sets_state_inner(function, &mut seen)
    }

    fn sets_state_inner(&self, function: &str, seen: &mut BTreeSet<String>) -> bool {
        if !seen.insert(function.to_owned()) {
            return false;
        }
        let Some(f) = self.functions.get(function) else {
            return false;
        };
        let body = self.tokens.get(f.body.clone()).unwrap_or_default();
        for (i, t) in body.iter().enumerate() {
            let at = |k: usize| body.get(i + k);
            if let Tok::Name(lib) = &t.tok {
                let field = match (at(1), at(2)) {
                    (
                        Some(dot),
                        Some(Token {
                            tok: Tok::Name(f), ..
                        }),
                    ) if dot.is_punct('.') => Some(f.as_str()),
                    _ => None,
                };
                if field.is_some_and(|f| STATE_SETTERS.contains(&(lib.as_str(), f))) {
                    return true;
                }
                let prev_is_field = i
                    .checked_sub(1)
                    .and_then(|p| body.get(p))
                    .is_some_and(|p| p.is_punct('.') || p.is_punct(':'));
                let called = at(1).is_some_and(|x| x.is_punct('('));
                if called && !prev_is_field && lib != function && self.sets_state_inner(lib, seen) {
                    return true;
                }
            }
        }
        false
    }

    /// String literals starting with `prefix`, with their byte ranges.
    pub fn strings_with_prefix<'a>(
        &'a self,
        prefix: &'a str,
    ) -> impl Iterator<Item = (&'a str, Range<usize>)> + 'a {
        self.strings
            .iter()
            .filter(move |(s, _)| s.starts_with(prefix))
            .map(|(s, r)| (s.as_str(), r.clone()))
    }
}

fn is_keyword(name: &str) -> bool {
    matches!(
        name,
        "and"
            | "break"
            | "do"
            | "else"
            | "elseif"
            | "end"
            | "false"
            | "for"
            | "function"
            | "goto"
            | "if"
            | "in"
            | "local"
            | "nil"
            | "not"
            | "or"
            | "repeat"
            | "return"
            | "then"
            | "true"
            | "until"
            | "while"
    )
}

/// Index one past the `end` closing the block opened by the `function` token at `start`.
fn block_end(tokens: &[Token], start: usize) -> usize {
    let mut depth = 0usize;
    for (i, t) in tokens.iter().enumerate().skip(start) {
        match &t.tok {
            Tok::Name(n) if matches!(n.as_str(), "function" | "do" | "if" | "repeat") => depth += 1,
            Tok::Name(n) if matches!(n.as_str(), "end" | "until") => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
    }
    tokens.len()
}

/// Level of a long bracket starting at `i` (`[[` → 0, `[==[` → 2), if one starts there.
fn long_bracket(bytes: &[u8], i: usize) -> Option<usize> {
    if bytes.get(i) != Some(&b'[') {
        return None;
    }
    let level = bytes[i + 1..].iter().take_while(|&&b| b == b'=').count();
    (bytes.get(i + 1 + level) == Some(&b'[')).then_some(level)
}

/// End (exclusive) of the long bracket of `level` whose content starts at `from`.
fn long_bracket_end(source: &str, from: usize, level: usize) -> (usize, usize) {
    let close = format!("]{}]", "=".repeat(level));
    match source.get(from..).and_then(|rest| rest.find(&close)) {
        Some(off) => (from + off, from + off + close.len()),
        None => (source.len(), source.len()),
    }
}

fn tokenize(source: &str) -> Vec<Token> {
    let bytes = source.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b.is_ascii_whitespace() {
            i += 1;
        } else if b == b'-' && bytes.get(i + 1) == Some(&b'-') {
            // Comment: long bracket or to the end of the line.
            match long_bracket(bytes, i + 2) {
                Some(level) => i = long_bracket_end(source, i + 4 + level, level).1,
                None => i = source[i..].find('\n').map_or(bytes.len(), |n| i + n),
            }
        } else if let Some(level) = long_bracket(bytes, i) {
            let content = i + 2 + level;
            let (end, after) = long_bracket_end(source, content, level);
            let text = source.get(content..end).unwrap_or_default();
            let text = text.strip_prefix('\n').unwrap_or(text);
            out.push(Token {
                tok: Tok::Str(text.to_owned()),
                range: i..after,
            });
            i = after;
        } else if b == b'"' || b == b'\'' {
            let (value, end) = quoted(source, i);
            out.push(Token {
                tok: Tok::Str(value),
                range: i..end,
            });
            i = end;
        } else if b.is_ascii_alphabetic() || b == b'_' {
            let len = bytes[i..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'_')
                .count();
            out.push(Token {
                tok: Tok::Name(source[i..i + len].to_owned()),
                range: i..i + len,
            });
            i += len;
        } else if b.is_ascii_digit() {
            let len = bytes[i..]
                .iter()
                .take_while(|c| c.is_ascii_alphanumeric() || **c == b'.')
                .count();
            out.push(Token {
                tok: Tok::Other,
                range: i..i + len,
            });
            i += len;
        } else {
            let ch = source[i..].chars().next().unwrap_or(' ');
            out.push(Token {
                tok: Tok::Punct(ch),
                range: i..i + ch.len_utf8(),
            });
            i += ch.len_utf8();
        }
    }
    out
}

/// A quoted string starting at `start`: its value (common escapes decoded) and the end
/// offset (after the closing quote, or the end of the line when unterminated).
fn quoted(source: &str, start: usize) -> (String, usize) {
    let quote = source.as_bytes()[start];
    let mut value = String::new();
    let mut chars = source[start + 1..].char_indices();
    while let Some((off, c)) = chars.next() {
        let at = start + 1 + off;
        match c {
            '\\' => match chars.next() {
                Some((_, 'n')) => value.push('\n'),
                Some((_, 't')) => value.push('\t'),
                Some((_, other)) => value.push(other),
                None => return (value, source.len()),
            },
            '\n' => return (value, at),
            c if c as u32 == u32::from(quote) => return (value, at + 1),
            c => value.push(c),
        }
    }
    (value, source.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
-- function commented_out() end
--[[ function also_commented() end ]]
local helper = require("helper")
local function private() end

function old_keeper(npc)
  if not game.flag("keeper_met") then
    ui.say("text:keeper.hello")
    remember()
  end
end

remember = function()
  for i = 1, 2 do end
  game.set_flag("keeper_met", true)
end

function rival_intro()
  ui.say('text:rival.hi')
  while true do break end
end

events.on("tick", function() end)
events.on("map_enter", function() end)
local s = [[text:not.a.call]]
data.register_move({ id = "zap" })
"#;

    #[test]
    fn finds_global_functions_only() {
        let facts = LuaFacts::scan(SRC);
        let names: Vec<&str> = facts.functions.keys().map(String::as_str).collect();
        assert_eq!(names, ["old_keeper", "remember", "rival_intro"]);
        let f = &facts.functions["old_keeper"];
        assert_eq!(&SRC[f.name.clone()], "old_keeper");
    }

    #[test]
    fn state_setters_are_followed_through_local_calls() {
        let facts = LuaFacts::scan(SRC);
        assert!(facts.sets_state("remember"));
        assert!(facts.sets_state("old_keeper"), "through remember()");
        assert!(!facts.sets_state("rival_intro"));
        assert!(!facts.sets_state("missing"));
    }

    #[test]
    fn tick_listeners_text_literals_and_registrations() {
        let facts = LuaFacts::scan(SRC);
        assert_eq!(facts.tick_listeners.len(), 1);
        assert_eq!(&SRC[facts.tick_listeners[0].clone()], "\"tick\"");
        let keys: Vec<&str> = facts.strings_with_prefix("text:").map(|(s, _)| s).collect();
        assert_eq!(
            keys,
            ["text:keeper.hello", "text:rival.hi", "text:not.a.call"]
        );
        assert!(facts.registers_content);
        assert!(facts.strings.iter().any(|(s, _)| s == "zap"));
    }

    #[test]
    fn strings_decode_escapes_and_survive_unterminated_input() {
        let facts = LuaFacts::scan("x = 'it\\'s'\ny = \"open\n");
        let values: Vec<&str> = facts.strings.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(values, ["it's", "open"]);
    }
}
