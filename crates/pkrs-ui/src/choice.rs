//! Cursor choice list with menu repeat (§5.5).

use pkrs_platform::{Action, BuiltinAction, Input, MenuCursorDirection};

use crate::UiError;

/// One selectable row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceOption {
    /// Label shown to the player.
    pub label: String,
}

/// Outcome of [`ChoiceSession::poll`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChoicePollOutcome {
    /// Row chosen with Confirm.
    Picked(usize),
    /// Back / cancel.
    Cancelled,
}

/// Vertical menu: Up/Down with repeat, Confirm to pick.
#[derive(Clone, Debug)]
pub struct ChoiceSession {
    options: Vec<ChoiceOption>,
    cursor: usize,
}

impl ChoiceSession {
    /// At least one option required by caller.
    pub fn new(options: Vec<ChoiceOption>) -> Result<Self, UiError> {
        if options.is_empty() {
            return Err(UiError::EmptyChoice);
        }
        Ok(Self { options, cursor: 0 })
    }

    /// Current highlighted row index (0-based).
    #[must_use]
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Moves the highlight to `cursor` (clamped to the list).
    pub fn set_cursor(&mut self, cursor: usize) {
        let last = self.options.len().saturating_sub(1);
        self.cursor = cursor.min(last);
    }

    /// All selectable options (order matches display).
    #[must_use]
    pub fn options(&self) -> &[ChoiceOption] {
        &self.options
    }

    /// Update cursor from `input`; returns a [`ChoicePollOutcome`] on confirm or cancel.
    pub fn poll(&mut self, input: &Input) -> Option<ChoicePollOutcome> {
        if let Some(dir) = input.menu_cursor_direction() {
            match dir {
                MenuCursorDirection::Up => {
                    self.cursor = self.cursor.saturating_sub(1);
                }
                MenuCursorDirection::Down => {
                    self.cursor = (self.cursor + 1).min(self.options.len() - 1);
                }
            }
        }
        if input.triggered(Action::Builtin(BuiltinAction::Confirm)) {
            return Some(ChoicePollOutcome::Picked(self.cursor));
        }
        if input.triggered(Action::Builtin(BuiltinAction::Cancel)) {
            return Some(ChoicePollOutcome::Cancelled);
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
    fn down_moves_cursor() {
        let mut input = Input::new(InputFacadeConfig::default());
        let mut plat = Platform::new(
            Box::new(NullPlatform::new().with_frames([NullFrame::with_keys([Key::Down])])),
            Box::new(FixedPaths::under("t")),
            PlatformConfig::default(),
        );
        block_on(plat.next_frame());
        input.poll(&plat);
        input.begin_tick();
        let mut c = ChoiceSession::new(vec![
            ChoiceOption { label: "a".into() },
            ChoiceOption { label: "b".into() },
        ])
        .unwrap();
        let _ = c.poll(&input);
        assert_eq!(c.cursor(), 1);
        let mut plat2 = Platform::new(
            Box::new(NullPlatform::new().with_frames([NullFrame::with_keys([Key::Enter])])),
            Box::new(FixedPaths::under("t")),
            PlatformConfig::default(),
        );
        block_on(plat2.next_frame());
        input.poll(&plat2);
        input.begin_tick();
        assert_eq!(c.poll(&input), Some(ChoicePollOutcome::Picked(1)));
    }
}
