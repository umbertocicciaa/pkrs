//! Dev `` ` `` console: evaluate a Lua expression (§15.1).

use std::collections::BTreeSet;

use pkrs_platform::Key;
use pkrs_script::{ScriptHost, ScriptOwner, ScriptValue};

/// Single-line Lua REPL shown when the console is open.
#[derive(Clone, Debug, Default)]
pub struct DevConsole {
    open: bool,
    line: String,
    last_result: String,
    last_error: String,
}

impl DevConsole {
    /// Empty console.
    pub fn new() -> Self {
        Self::default()
    }

    /// `true` while the console captures keyboard input.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Current input line.
    pub fn line(&self) -> &str {
        &self.line
    }

    /// Last result or error text for the overlay.
    pub fn status_text(&self) -> Option<&str> {
        if !self.last_error.is_empty() {
            Some(&self.last_error)
        } else if !self.last_result.is_empty() {
            Some(&self.last_result)
        } else {
            None
        }
    }

    /// Handles keys; returns `true` when Enter should run the line.
    pub fn poll_keys(&mut self, keys: &BTreeSet<Key>, prev: &BTreeSet<Key>) -> bool {
        let edge = |k: Key| keys.contains(&k) && !prev.contains(&k);
        if edge(Key::Backquote) {
            self.open = !self.open;
            if !self.open {
                self.line.clear();
            }
        }
        if !self.open {
            return false;
        }
        if edge(Key::Escape) {
            self.open = false;
            self.line.clear();
            return false;
        }
        if edge(Key::Backspace) {
            self.line.pop();
            return false;
        }
        if edge(Key::Enter) {
            return !self.line.trim().is_empty();
        }
        for key in keys.difference(prev) {
            if let Some(ch) = key_char(*key) {
                self.line.push(ch);
            }
        }
        false
    }

    /// Runs `line` as Lua in the game owner and stores the formatted result.
    pub fn execute<H: ScriptHost + ?Sized>(&mut self, host: &mut H, line: &str) {
        self.last_error.clear();
        self.last_result.clear();
        let source = if line.trim_start().starts_with("return") {
            line.to_owned()
        } else {
            format!("return ({line})")
        };
        match host.run_source(&ScriptOwner::Game, "console", &source) {
            Ok(v) => self.last_result = format_value(&v),
            Err(e) => self.last_error = e.to_string(),
        }
        self.line.clear();
    }
}

fn format_value(value: &ScriptValue) -> String {
    match value {
        ScriptValue::Nil => "nil".into(),
        ScriptValue::Bool(b) => b.to_string(),
        ScriptValue::Int(i) => i.to_string(),
        ScriptValue::Number(n) => n.to_string(),
        ScriptValue::String(s) => s.clone(),
        other => format!("{other:?}"),
    }
}

fn key_char(key: Key) -> Option<char> {
    match key {
        Key::A => Some('a'),
        Key::B => Some('b'),
        Key::C => Some('c'),
        Key::D => Some('d'),
        Key::E => Some('e'),
        Key::F => Some('f'),
        Key::G => Some('g'),
        Key::H => Some('h'),
        Key::I => Some('i'),
        Key::J => Some('j'),
        Key::K => Some('k'),
        Key::L => Some('l'),
        Key::M => Some('m'),
        Key::N => Some('n'),
        Key::O => Some('o'),
        Key::P => Some('p'),
        Key::Q => Some('q'),
        Key::R => Some('r'),
        Key::S => Some('s'),
        Key::T => Some('t'),
        Key::U => Some('u'),
        Key::V => Some('v'),
        Key::W => Some('w'),
        Key::X => Some('x'),
        Key::Y => Some('y'),
        Key::Z => Some('z'),
        Key::Digit0 => Some('0'),
        Key::Digit1 => Some('1'),
        Key::Digit2 => Some('2'),
        Key::Digit3 => Some('3'),
        Key::Digit4 => Some('4'),
        Key::Digit5 => Some('5'),
        Key::Digit6 => Some('6'),
        Key::Digit7 => Some('7'),
        Key::Digit8 => Some('8'),
        Key::Digit9 => Some('9'),
        Key::Space => Some(' '),
        Key::Period => Some('.'),
        Key::Comma => Some(','),
        Key::Minus => Some('-'),
        Key::Slash => Some('/'),
        Key::Semicolon => Some(';'),
        Key::Apostrophe => Some('\''),
        Key::Equal => Some('='),
        _ => None,
    }
}
