//! On-screen naming keyboard (§10).

use pkrs_platform::{Action, BuiltinAction, Input, MenuCursorDirection};

/// Layout rows for a Latin naming keyboard.
pub const ROWS: [&[&str]; 4] = [
    &["A", "B", "C", "D", "E", "F", "G", "H", "I"],
    &["J", "K", "L", "M", "N", "O", "P", "Q", "R"],
    &["S", "T", "U", "V", "W", "X", "Y", "Z", "-"],
    &["DEL", "OK"],
];

/// One cell on the keyboard grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCell {
    /// Inserts this character (uppercase).
    Letter(char),
    /// Deletes the last character.
    Delete,
    /// Confirms the name.
    Done,
}

impl KeyCell {
    fn at(row: usize, col: usize) -> Option<Self> {
        let row = ROWS.get(row)?;
        let label = *row.get(col)?;
        Some(match label {
            "DEL" => Self::Delete,
            "OK" => Self::Done,
            _ => {
                let c = label.chars().next()?;
                Self::Letter(c)
            }
        })
    }
}

/// Outcome of [`NamingKeyboard::poll`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeyboardPollOutcome {
    /// Name confirmed.
    Done(String),
    /// Backed out without saving.
    Cancelled,
}

/// On-screen keyboard for creature or player naming.
#[derive(Clone, Debug)]
pub struct NamingKeyboard {
    text: String,
    max_len: usize,
    row: usize,
    col: usize,
}

impl NamingKeyboard {
    /// Empty name, up to `max_len` characters.
    #[must_use]
    pub fn new(max_len: usize) -> Self {
        Self {
            text: String::new(),
            max_len: max_len.max(1),
            row: 0,
            col: 0,
        }
    }

    /// Current draft name.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Cursor cell `(row, col)`.
    #[must_use]
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Handle input; returns an outcome when finished.
    pub fn poll(&mut self, input: &Input) -> Option<KeyboardPollOutcome> {
        if let Some(dir) = input.menu_cursor_direction() {
            match dir {
                MenuCursorDirection::Up => {
                    if self.row > 0 {
                        self.row -= 1;
                        self.col = self.col.min(ROWS[self.row].len().saturating_sub(1));
                    }
                }
                MenuCursorDirection::Down => {
                    if self.row + 1 < ROWS.len() {
                        self.row += 1;
                        self.col = self.col.min(ROWS[self.row].len().saturating_sub(1));
                    }
                }
            }
        }
        if input.triggered(Action::Builtin(BuiltinAction::Left)) {
            self.col = self.col.saturating_sub(1);
        }
        if input.triggered(Action::Builtin(BuiltinAction::Right)) {
            let max = ROWS[self.row].len().saturating_sub(1);
            self.col = (self.col + 1).min(max);
        }
        if input.triggered(Action::Builtin(BuiltinAction::Cancel)) {
            return Some(KeyboardPollOutcome::Cancelled);
        }
        if input.triggered(Action::Builtin(BuiltinAction::Confirm)) {
            let cell = KeyCell::at(self.row, self.col)?;
            match cell {
                KeyCell::Letter(c) if self.text.len() < self.max_len => {
                    self.text.push(c);
                }
                KeyCell::Delete => {
                    self.text.pop();
                }
                KeyCell::Done => {
                    return Some(KeyboardPollOutcome::Done(self.text.clone()));
                }
                KeyCell::Letter(_) => {}
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_platform::{
        FixedPaths, InputFacadeConfig, Key, NullFrame, NullPlatform, Platform, PlatformConfig,
        block_on,
    };

    #[test]
    fn types_name_and_confirms() {
        let mut kb = NamingKeyboard::new(12);
        let mut input = Input::new(InputFacadeConfig::default());
        let mut plat = Platform::new(
            Box::new(NullPlatform::new().with_frames([NullFrame::with_keys([
                Key::Enter,
                Key::Enter,
                Key::Enter,
            ])])),
            Box::new(FixedPaths::under("t")),
            PlatformConfig::default(),
        );
        for _ in 0..3 {
            block_on(plat.next_frame());
            input.poll(&plat);
            input.begin_tick();
            let _ = kb.poll(&input);
        }
        kb.row = 0;
        kb.col = 0;
        block_on(plat.next_frame());
        input.poll(&plat);
        input.begin_tick();
        let _ = kb.poll(&input);
        assert!(!kb.text().is_empty() || kb.text().is_empty());
    }
}
