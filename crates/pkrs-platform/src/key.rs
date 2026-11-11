//! Raw keyboard keys in engine vocabulary (§4.5 Rule 1: no vendor key codes).
//!
//! These are *physical* keys. Game code reads actions, never keys (§10); the
//! [`Input`](crate::Input) facade maps keys to actions.

/// A physical keyboard key known to the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum Key {
    /// The A key.
    A,
    /// The B key.
    B,
    /// The C key.
    C,
    /// The D key.
    D,
    /// The E key.
    E,
    /// The F key.
    F,
    /// The G key.
    G,
    /// The H key.
    H,
    /// The I key.
    I,
    /// The J key.
    J,
    /// The K key.
    K,
    /// The L key.
    L,
    /// The M key.
    M,
    /// The N key.
    N,
    /// The O key.
    O,
    /// The P key.
    P,
    /// The Q key.
    Q,
    /// The R key.
    R,
    /// The S key.
    S,
    /// The T key.
    T,
    /// The U key.
    U,
    /// The V key.
    V,
    /// The W key.
    W,
    /// The X key.
    X,
    /// The Y key.
    Y,
    /// The Z key.
    Z,
    /// The 0 key.
    Digit0,
    /// The 1 key.
    Digit1,
    /// The 2 key.
    Digit2,
    /// The 3 key.
    Digit3,
    /// The 4 key.
    Digit4,
    /// The 5 key.
    Digit5,
    /// The 6 key.
    Digit6,
    /// The 7 key.
    Digit7,
    /// The 8 key.
    Digit8,
    /// The 9 key.
    Digit9,
    /// The up arrow key.
    Up,
    /// The down arrow key.
    Down,
    /// The left arrow key.
    Left,
    /// The right arrow key.
    Right,
    /// The space bar.
    Space,
    /// The Enter key.
    Enter,
    /// The Escape key.
    Escape,
    /// The Backspace key.
    Backspace,
    /// The Tab key.
    Tab,
    /// The Insert key.
    Insert,
    /// The Delete key.
    Delete,
    /// The Home key.
    Home,
    /// The End key.
    End,
    /// The Page Up key.
    PageUp,
    /// The Page Down key.
    PageDown,
    /// The left Shift key.
    LeftShift,
    /// The right Shift key.
    RightShift,
    /// The left Control key.
    LeftControl,
    /// The right Control key.
    RightControl,
    /// The left Alt key.
    LeftAlt,
    /// The right Alt key.
    RightAlt,
    /// The `` ` `` / `~` key (opens the dev console, §15.1).
    Backquote,
    /// The `-` key.
    Minus,
    /// The `=` key.
    Equal,
    /// The `,` key.
    Comma,
    /// The `.` key.
    Period,
    /// The `/` key.
    Slash,
    /// The `;` key.
    Semicolon,
    /// The `'` key.
    Apostrophe,
    /// The `[` key.
    LeftBracket,
    /// The `]` key.
    RightBracket,
    /// The `\` key.
    Backslash,
    /// The F1 key.
    F1,
    /// The F2 key.
    F2,
    /// The F3 key.
    F3,
    /// The F4 key.
    F4,
    /// The F5 key.
    F5,
    /// The F6 key.
    F6,
    /// The F7 key.
    F7,
    /// The F8 key.
    F8,
    /// The F9 key.
    F9,
    /// The F10 key.
    F10,
    /// The F11 key.
    F11,
    /// The F12 key.
    F12,
}

impl Key {
    /// Every key, in declaration order.
    pub const ALL: &'static [Key] = &[
        Key::A,
        Key::B,
        Key::C,
        Key::D,
        Key::E,
        Key::F,
        Key::G,
        Key::H,
        Key::I,
        Key::J,
        Key::K,
        Key::L,
        Key::M,
        Key::N,
        Key::O,
        Key::P,
        Key::Q,
        Key::R,
        Key::S,
        Key::T,
        Key::U,
        Key::V,
        Key::W,
        Key::X,
        Key::Y,
        Key::Z,
        Key::Digit0,
        Key::Digit1,
        Key::Digit2,
        Key::Digit3,
        Key::Digit4,
        Key::Digit5,
        Key::Digit6,
        Key::Digit7,
        Key::Digit8,
        Key::Digit9,
        Key::Up,
        Key::Down,
        Key::Left,
        Key::Right,
        Key::Space,
        Key::Enter,
        Key::Escape,
        Key::Backspace,
        Key::Tab,
        Key::Insert,
        Key::Delete,
        Key::Home,
        Key::End,
        Key::PageUp,
        Key::PageDown,
        Key::LeftShift,
        Key::RightShift,
        Key::LeftControl,
        Key::RightControl,
        Key::LeftAlt,
        Key::RightAlt,
        Key::Backquote,
        Key::Minus,
        Key::Equal,
        Key::Comma,
        Key::Period,
        Key::Slash,
        Key::Semicolon,
        Key::Apostrophe,
        Key::LeftBracket,
        Key::RightBracket,
        Key::Backslash,
        Key::F1,
        Key::F2,
        Key::F3,
        Key::F4,
        Key::F5,
        Key::F6,
        Key::F7,
        Key::F8,
        Key::F9,
        Key::F10,
        Key::F11,
        Key::F12,
    ];

    /// Parses a manifest / config key name (`w`, `left_shift`, `enter`, …).
    pub fn parse_name(raw: &str) -> Option<Self> {
        let name = raw.trim();
        if name.len() == 1 {
            let c = name.as_bytes()[0];
            if c.is_ascii_lowercase() {
                return single_letter(c);
            }
            if c.is_ascii_digit() {
                return single_digit(c);
            }
        }
        match name {
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            "space" => Some(Key::Space),
            "enter" | "return" => Some(Key::Enter),
            "escape" | "esc" => Some(Key::Escape),
            "backspace" => Some(Key::Backspace),
            "tab" => Some(Key::Tab),
            "insert" => Some(Key::Insert),
            "delete" => Some(Key::Delete),
            "home" => Some(Key::Home),
            "end" => Some(Key::End),
            "pageup" | "page_up" => Some(Key::PageUp),
            "pagedown" | "page_down" => Some(Key::PageDown),
            "left_shift" | "shift" => Some(Key::LeftShift),
            "right_shift" => Some(Key::RightShift),
            "left_control" | "left_ctrl" | "control" | "ctrl" => Some(Key::LeftControl),
            "right_control" | "right_ctrl" => Some(Key::RightControl),
            "left_alt" | "alt" => Some(Key::LeftAlt),
            "right_alt" => Some(Key::RightAlt),
            "backquote" | "grave" => Some(Key::Backquote),
            "minus" => Some(Key::Minus),
            "equal" => Some(Key::Equal),
            "comma" => Some(Key::Comma),
            "period" => Some(Key::Period),
            "slash" => Some(Key::Slash),
            "semicolon" => Some(Key::Semicolon),
            "apostrophe" | "quote" => Some(Key::Apostrophe),
            "left_bracket" => Some(Key::LeftBracket),
            "right_bracket" => Some(Key::RightBracket),
            "backslash" => Some(Key::Backslash),
            "f1" => Some(Key::F1),
            "f2" => Some(Key::F2),
            "f3" => Some(Key::F3),
            "f4" => Some(Key::F4),
            "f5" => Some(Key::F5),
            "f6" => Some(Key::F6),
            "f7" => Some(Key::F7),
            "f8" => Some(Key::F8),
            "f9" => Some(Key::F9),
            "f10" => Some(Key::F10),
            "f11" => Some(Key::F11),
            "f12" => Some(Key::F12),
            _ => None,
        }
    }

    /// Canonical config name. [`parse_name`](Self::parse_name) accepts it.
    #[must_use]
    pub const fn config_name(self) -> &'static str {
        match self {
            Self::A => "a",
            Self::B => "b",
            Self::C => "c",
            Self::D => "d",
            Self::E => "e",
            Self::F => "f",
            Self::G => "g",
            Self::H => "h",
            Self::I => "i",
            Self::J => "j",
            Self::K => "k",
            Self::L => "l",
            Self::M => "m",
            Self::N => "n",
            Self::O => "o",
            Self::P => "p",
            Self::Q => "q",
            Self::R => "r",
            Self::S => "s",
            Self::T => "t",
            Self::U => "u",
            Self::V => "v",
            Self::W => "w",
            Self::X => "x",
            Self::Y => "y",
            Self::Z => "z",
            Self::Digit0 => "0",
            Self::Digit1 => "1",
            Self::Digit2 => "2",
            Self::Digit3 => "3",
            Self::Digit4 => "4",
            Self::Digit5 => "5",
            Self::Digit6 => "6",
            Self::Digit7 => "7",
            Self::Digit8 => "8",
            Self::Digit9 => "9",
            Self::Up => "up",
            Self::Down => "down",
            Self::Left => "left",
            Self::Right => "right",
            Self::Space => "space",
            Self::Enter => "enter",
            Self::Escape => "escape",
            Self::Backspace => "backspace",
            Self::Tab => "tab",
            Self::Insert => "insert",
            Self::Delete => "delete",
            Self::Home => "home",
            Self::End => "end",
            Self::PageUp => "pageup",
            Self::PageDown => "pagedown",
            Self::LeftShift => "left_shift",
            Self::RightShift => "right_shift",
            Self::LeftControl => "left_control",
            Self::RightControl => "right_control",
            Self::LeftAlt => "left_alt",
            Self::RightAlt => "right_alt",
            Self::Backquote => "backquote",
            Self::Minus => "minus",
            Self::Equal => "equal",
            Self::Comma => "comma",
            Self::Period => "period",
            Self::Slash => "slash",
            Self::Semicolon => "semicolon",
            Self::Apostrophe => "apostrophe",
            Self::LeftBracket => "left_bracket",
            Self::RightBracket => "right_bracket",
            Self::Backslash => "backslash",
            Self::F1 => "f1",
            Self::F2 => "f2",
            Self::F3 => "f3",
            Self::F4 => "f4",
            Self::F5 => "f5",
            Self::F6 => "f6",
            Self::F7 => "f7",
            Self::F8 => "f8",
            Self::F9 => "f9",
            Self::F10 => "f10",
            Self::F11 => "f11",
            Self::F12 => "f12",
        }
    }
}

fn single_letter(c: u8) -> Option<Key> {
    Some(match c {
        b'a' => Key::A,
        b'b' => Key::B,
        b'c' => Key::C,
        b'd' => Key::D,
        b'e' => Key::E,
        b'f' => Key::F,
        b'g' => Key::G,
        b'h' => Key::H,
        b'i' => Key::I,
        b'j' => Key::J,
        b'k' => Key::K,
        b'l' => Key::L,
        b'm' => Key::M,
        b'n' => Key::N,
        b'o' => Key::O,
        b'p' => Key::P,
        b'q' => Key::Q,
        b'r' => Key::R,
        b's' => Key::S,
        b't' => Key::T,
        b'u' => Key::U,
        b'v' => Key::V,
        b'w' => Key::W,
        b'x' => Key::X,
        b'y' => Key::Y,
        b'z' => Key::Z,
        _ => return None,
    })
}

fn single_digit(c: u8) -> Option<Key> {
    Some(match c {
        b'0' => Key::Digit0,
        b'1' => Key::Digit1,
        b'2' => Key::Digit2,
        b'3' => Key::Digit3,
        b'4' => Key::Digit4,
        b'5' => Key::Digit5,
        b'6' => Key::Digit6,
        b'7' => Key::Digit7,
        b'8' => Key::Digit8,
        b'9' => Key::Digit9,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn all_is_sorted_and_unique() {
        let set: BTreeSet<Key> = Key::ALL.iter().copied().collect();
        assert_eq!(set.len(), Key::ALL.len());
        assert!(Key::ALL.windows(2).all(|w| w[0] < w[1]));
    }
}
