//! Standard gamepad buttons in engine vocabulary (§4.5, §10).
//!
//! These are the faces, d-pad, shoulders and left-stick directions of one pad.
//! Game code reads actions, never buttons; the [`Input`](crate::Input) facade maps them.

use core::fmt;

/// A button on the standard gamepad mapping (the first connected pad, §10).
///
/// Stick directions are digital: the adapter sets them when the axis passes the deadzone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum GamepadButton {
    /// Bottom face button (Xbox A / PlayStation Cross). Confirm by default.
    South,
    /// Right face button (Xbox B / PlayStation Circle). Cancel by default.
    East,
    /// Left face button (Xbox X / PlayStation Square).
    West,
    /// Top face button (Xbox Y / PlayStation Triangle).
    North,
    /// D-pad up.
    DPadUp,
    /// D-pad down.
    DPadDown,
    /// D-pad left.
    DPadLeft,
    /// D-pad right.
    DPadRight,
    /// Left bumper.
    LeftShoulder,
    /// Right bumper. Run by default.
    RightShoulder,
    /// Left trigger, as a digital press.
    LeftTrigger,
    /// Right trigger, as a digital press.
    RightTrigger,
    /// Back / Select.
    Select,
    /// Start / Menu.
    Start,
    /// Left stick click.
    LeftStick,
    /// Right stick click.
    RightStick,
    /// Left stick pushed up (past the deadzone).
    LeftStickUp,
    /// Left stick pushed down.
    LeftStickDown,
    /// Left stick pushed left.
    LeftStickLeft,
    /// Left stick pushed right.
    LeftStickRight,
}

impl GamepadButton {
    /// Every button, in declaration order.
    pub const ALL: &'static [GamepadButton] = &[
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::DPadUp,
        Self::DPadDown,
        Self::DPadLeft,
        Self::DPadRight,
        Self::LeftShoulder,
        Self::RightShoulder,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::Select,
        Self::Start,
        Self::LeftStick,
        Self::RightStick,
        Self::LeftStickUp,
        Self::LeftStickDown,
        Self::LeftStickLeft,
        Self::LeftStickRight,
    ];

    /// Config / screen name (`south`, `dpad_up`, `stick_left`, …).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::South => "south",
            Self::East => "east",
            Self::West => "west",
            Self::North => "north",
            Self::DPadUp => "dpad_up",
            Self::DPadDown => "dpad_down",
            Self::DPadLeft => "dpad_left",
            Self::DPadRight => "dpad_right",
            Self::LeftShoulder => "left_shoulder",
            Self::RightShoulder => "right_shoulder",
            Self::LeftTrigger => "left_trigger",
            Self::RightTrigger => "right_trigger",
            Self::Select => "select",
            Self::Start => "start",
            Self::LeftStick => "left_stick",
            Self::RightStick => "right_stick",
            Self::LeftStickUp => "stick_up",
            Self::LeftStickDown => "stick_down",
            Self::LeftStickLeft => "stick_left",
            Self::LeftStickRight => "stick_right",
        }
    }

    /// Parses [`name`](Self::name).
    #[must_use]
    pub fn parse_name(raw: &str) -> Option<Self> {
        let name = raw.trim();
        Self::ALL
            .iter()
            .copied()
            .find(|button| button.name() == name)
    }
}

impl fmt::Display for GamepadButton {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for button in GamepadButton::ALL {
            assert_eq!(GamepadButton::parse_name(button.name()), Some(*button));
        }
        assert_eq!(GamepadButton::parse_name("nope"), None);
    }
}
