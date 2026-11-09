//! `GilrsGamepad`: the `gilrs` adapter for [`GamepadSource`](crate::GamepadSource) (§4.5, §10).
//!
//! This is the only module that names `gilrs`. Buttons leave as [`GamepadButton`](crate::GamepadButton).
//! The first connected pad is the active one; its left stick becomes digital directions
//! past a fixed deadzone.

use crate::button::GamepadButton;
use crate::gamepad::GamepadSource;

/// Stick deflection that counts as a direction. Below this, the stick adds no button.
const STICK_DEADZONE: f32 = 0.5;

/// Gamepad adapter over `gilrs`. A failed open behaves as no pad connected.
pub struct GilrsGamepad {
    inner: Option<gilrs::Gilrs>,
    /// Why [`open`](Self::open) has no device. `None` when `gilrs` started.
    error: Option<String>,
}

impl GilrsGamepad {
    /// Starts `gilrs`. When the backend cannot open, buttons stay empty and
    /// [`error`](Self::error) explains why (logged by the runtime).
    #[must_use]
    pub fn open() -> Self {
        match gilrs::Gilrs::new() {
            Ok(gilrs) => Self {
                inner: Some(gilrs),
                error: None,
            },
            Err(err) => Self {
                inner: None,
                error: Some(err.to_string()),
            },
        }
    }

    /// The open failure, if [`open`](Self::open) could not start `gilrs`.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

impl GamepadSource for GilrsGamepad {
    fn poll(&mut self) {
        let Some(gilrs) = self.inner.as_mut() else {
            return;
        };
        while gilrs.next_event().is_some() {}
    }

    fn buttons_down(&self) -> Vec<GamepadButton> {
        let Some(gilrs) = self.inner.as_ref() else {
            return Vec::new();
        };
        let Some((_, pad)) = gilrs.gamepads().next() else {
            return Vec::new();
        };
        let mut buttons = Vec::new();
        for (vendor, button) in MAPPED {
            if pad.is_pressed(*vendor) {
                buttons.push(*button);
            }
        }
        let x = pad.value(gilrs::Axis::LeftStickX);
        let y = pad.value(gilrs::Axis::LeftStickY);
        if x >= STICK_DEADZONE {
            buttons.push(GamepadButton::LeftStickRight);
        } else if x <= -STICK_DEADZONE {
            buttons.push(GamepadButton::LeftStickLeft);
        }
        // gilrs / SDL: positive left-stick Y is up.
        if y >= STICK_DEADZONE {
            buttons.push(GamepadButton::LeftStickUp);
        } else if y <= -STICK_DEADZONE {
            buttons.push(GamepadButton::LeftStickDown);
        }
        buttons
    }
}

const MAPPED: &[(gilrs::Button, GamepadButton)] = &[
    (gilrs::Button::South, GamepadButton::South),
    (gilrs::Button::East, GamepadButton::East),
    (gilrs::Button::West, GamepadButton::West),
    (gilrs::Button::North, GamepadButton::North),
    (gilrs::Button::DPadUp, GamepadButton::DPadUp),
    (gilrs::Button::DPadDown, GamepadButton::DPadDown),
    (gilrs::Button::DPadLeft, GamepadButton::DPadLeft),
    (gilrs::Button::DPadRight, GamepadButton::DPadRight),
    (gilrs::Button::LeftTrigger, GamepadButton::LeftShoulder),
    (gilrs::Button::RightTrigger, GamepadButton::RightShoulder),
    (gilrs::Button::LeftTrigger2, GamepadButton::LeftTrigger),
    (gilrs::Button::RightTrigger2, GamepadButton::RightTrigger),
    (gilrs::Button::Select, GamepadButton::Select),
    (gilrs::Button::Start, GamepadButton::Start),
    (gilrs::Button::LeftThumb, GamepadButton::LeftStick),
    (gilrs::Button::RightThumb, GamepadButton::RightStick),
];
