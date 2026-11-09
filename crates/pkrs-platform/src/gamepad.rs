//! The `GamepadSource` port and the scripted [`NullGamepad`] adapter (§4.5, §10).
//!
//! [`GilrsGamepad`](crate::GilrsGamepad) (feature `gilrs`) is the windowed adapter and the
//! only module that names the `gilrs` crate. Both adapters feed the [`Input`](crate::Input)
//! facade; the game never reads buttons itself.

use std::collections::VecDeque;

use crate::button::GamepadButton;

/// Engine-owned port over one gamepad.
///
/// [`poll`](Self::poll) refreshes the device once per rendered frame. [`buttons_down`](Self::buttons_down)
/// is the standard-mapping set held afterwards, including digital left-stick directions.
/// The first connected pad is the one that counts; further pads are ignored (§10).
pub trait GamepadSource {
    /// Pulls pending device events. Call once per frame, before [`buttons_down`](Self::buttons_down).
    fn poll(&mut self);

    /// Buttons held on the active pad after the last [`poll`](Self::poll).
    fn buttons_down(&self) -> Vec<GamepadButton>;
}

/// Scripted [`GamepadSource`] for tests and `--headless` (§15.2).
///
/// [`holding`](Self::holding) keeps one set for every poll. [`with_frames`](Self::with_frames)
/// advances one set per [`poll`](GamepadSource::poll), then holds the last set.
#[derive(Clone, Debug, Default)]
pub struct NullGamepad {
    script: VecDeque<Vec<GamepadButton>>,
    current: Vec<GamepadButton>,
}

impl NullGamepad {
    /// No pad held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reports `buttons` on every poll.
    #[must_use]
    pub fn holding(buttons: impl IntoIterator<Item = GamepadButton>) -> Self {
        Self {
            script: VecDeque::new(),
            current: buttons.into_iter().collect(),
        }
    }

    /// Each [`poll`](GamepadSource::poll) takes the next set. After the script ends, the last
    /// set stays held.
    #[must_use]
    pub fn with_frames<I>(frames: impl IntoIterator<Item = I>) -> Self
    where
        I: IntoIterator<Item = GamepadButton>,
    {
        Self {
            script: frames
                .into_iter()
                .map(|frame| frame.into_iter().collect())
                .collect(),
            current: Vec::new(),
        }
    }
}

impl GamepadSource for NullGamepad {
    fn poll(&mut self) {
        if let Some(next) = self.script.pop_front() {
            self.current = next;
        }
    }

    fn buttons_down(&self) -> Vec<GamepadButton> {
        self.current.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holding_survives_polls() {
        let mut pad = NullGamepad::holding([GamepadButton::DPadRight]);
        pad.poll();
        assert_eq!(pad.buttons_down(), vec![GamepadButton::DPadRight]);
        pad.poll();
        assert_eq!(pad.buttons_down(), vec![GamepadButton::DPadRight]);
    }

    #[test]
    fn frames_advance_then_hold_the_last() {
        let mut pad =
            NullGamepad::with_frames([vec![GamepadButton::South], vec![GamepadButton::East]]);
        assert!(pad.buttons_down().is_empty());
        pad.poll();
        assert_eq!(pad.buttons_down(), vec![GamepadButton::South]);
        pad.poll();
        assert_eq!(pad.buttons_down(), vec![GamepadButton::East]);
        pad.poll();
        assert_eq!(pad.buttons_down(), vec![GamepadButton::East]);
    }
}
