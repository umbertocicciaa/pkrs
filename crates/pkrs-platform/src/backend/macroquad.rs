//! `MacroquadPlatform`: the `PlatformBackend` adapter over macroquad (§4.5).
//!
//! This is the **only** module of `pkrs-platform` that imports `macroquad`.
//!
//! Focus detection uses miniquad's `window_minimized_event` / `window_restored_event`,
//! which miniquad raises on focus changes on X11, macOS and wasm. miniquad does not
//! report focus on Windows, so there the window is always treated as focused. As a
//! fallback for platforms that only report focus *loss*, any key press or mouse click
//! marks the window as focused again.

use std::future::Future;
use std::path::Path;

use ::macroquad::input::KeyCode;
use ::macroquad::input::utils::{register_input_subscriber, repeat_all_miniquad_input};
use ::macroquad::miniquad::{EventHandler, KeyMods, MouseButton};
use ::macroquad::window::Conf;

use super::{FrameFuture, PlatformBackend};
use crate::key::Key;
use crate::window::WindowConfig;
use crate::{PlatformError, Result};

/// One table drives both directions of the key mapping.
const KEY_MAP: &[(Key, KeyCode)] = &[
    (Key::A, KeyCode::A),
    (Key::B, KeyCode::B),
    (Key::C, KeyCode::C),
    (Key::D, KeyCode::D),
    (Key::E, KeyCode::E),
    (Key::F, KeyCode::F),
    (Key::G, KeyCode::G),
    (Key::H, KeyCode::H),
    (Key::I, KeyCode::I),
    (Key::J, KeyCode::J),
    (Key::K, KeyCode::K),
    (Key::L, KeyCode::L),
    (Key::M, KeyCode::M),
    (Key::N, KeyCode::N),
    (Key::O, KeyCode::O),
    (Key::P, KeyCode::P),
    (Key::Q, KeyCode::Q),
    (Key::R, KeyCode::R),
    (Key::S, KeyCode::S),
    (Key::T, KeyCode::T),
    (Key::U, KeyCode::U),
    (Key::V, KeyCode::V),
    (Key::W, KeyCode::W),
    (Key::X, KeyCode::X),
    (Key::Y, KeyCode::Y),
    (Key::Z, KeyCode::Z),
    (Key::Digit0, KeyCode::Key0),
    (Key::Digit1, KeyCode::Key1),
    (Key::Digit2, KeyCode::Key2),
    (Key::Digit3, KeyCode::Key3),
    (Key::Digit4, KeyCode::Key4),
    (Key::Digit5, KeyCode::Key5),
    (Key::Digit6, KeyCode::Key6),
    (Key::Digit7, KeyCode::Key7),
    (Key::Digit8, KeyCode::Key8),
    (Key::Digit9, KeyCode::Key9),
    (Key::Up, KeyCode::Up),
    (Key::Down, KeyCode::Down),
    (Key::Left, KeyCode::Left),
    (Key::Right, KeyCode::Right),
    (Key::Space, KeyCode::Space),
    (Key::Enter, KeyCode::Enter),
    (Key::Escape, KeyCode::Escape),
    (Key::Backspace, KeyCode::Backspace),
    (Key::Tab, KeyCode::Tab),
    (Key::Insert, KeyCode::Insert),
    (Key::Delete, KeyCode::Delete),
    (Key::Home, KeyCode::Home),
    (Key::End, KeyCode::End),
    (Key::PageUp, KeyCode::PageUp),
    (Key::PageDown, KeyCode::PageDown),
    (Key::LeftShift, KeyCode::LeftShift),
    (Key::RightShift, KeyCode::RightShift),
    (Key::LeftControl, KeyCode::LeftControl),
    (Key::RightControl, KeyCode::RightControl),
    (Key::LeftAlt, KeyCode::LeftAlt),
    (Key::RightAlt, KeyCode::RightAlt),
    (Key::Backquote, KeyCode::GraveAccent),
    (Key::Minus, KeyCode::Minus),
    (Key::Equal, KeyCode::Equal),
    (Key::Comma, KeyCode::Comma),
    (Key::Period, KeyCode::Period),
    (Key::Slash, KeyCode::Slash),
    (Key::Semicolon, KeyCode::Semicolon),
    (Key::Apostrophe, KeyCode::Apostrophe),
    (Key::LeftBracket, KeyCode::LeftBracket),
    (Key::RightBracket, KeyCode::RightBracket),
    (Key::Backslash, KeyCode::Backslash),
    (Key::F1, KeyCode::F1),
    (Key::F2, KeyCode::F2),
    (Key::F3, KeyCode::F3),
    (Key::F4, KeyCode::F4),
    (Key::F5, KeyCode::F5),
    (Key::F6, KeyCode::F6),
    (Key::F7, KeyCode::F7),
    (Key::F8, KeyCode::F8),
    (Key::F9, KeyCode::F9),
    (Key::F10, KeyCode::F10),
    (Key::F11, KeyCode::F11),
    (Key::F12, KeyCode::F12),
];

fn to_vendor(key: Key) -> Option<KeyCode> {
    KEY_MAP
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, code)| *code)
}

fn from_vendor(code: KeyCode) -> Option<Key> {
    KEY_MAP
        .iter()
        .find(|(_, c)| *c == code)
        .map(|(key, _)| *key)
}

fn to_conf(window: &WindowConfig) -> Conf {
    Conf {
        window_title: window.title.clone(),
        window_width: i32::try_from(window.width).unwrap_or(i32::MAX),
        window_height: i32::try_from(window.height).unwrap_or(i32::MAX),
        fullscreen: window.fullscreen,
        window_resizable: window.resizable,
        ..Conf::default()
    }
}

/// Receives the miniquad events macroquad replays to input subscribers.
struct FocusTracker {
    focused: bool,
}

impl EventHandler for FocusTracker {
    fn update(&mut self) {}
    fn draw(&mut self) {}

    fn window_minimized_event(&mut self) {
        self.focused = false;
    }

    fn window_restored_event(&mut self) {
        self.focused = true;
    }

    fn key_down_event(&mut self, _keycode: KeyCode, _keymods: KeyMods, _repeat: bool) {
        self.focused = true;
    }

    fn mouse_button_down_event(&mut self, _button: MouseButton, _x: f32, _y: f32) {
        self.focused = true;
    }
}

/// [`PlatformBackend`] backed by macroquad: window, main loop, keyboard and focus.
///
/// It can only exist inside the macroquad event loop, so it is obtained through
/// [`MacroquadPlatform::run`].
pub struct MacroquadPlatform {
    subscriber: usize,
    focus: FocusTracker,
}

impl MacroquadPlatform {
    /// Opens the window described by `window` and runs `main` inside macroquad's loop.
    ///
    /// Blocks until `main` completes and the window closes.
    pub fn run<F, Fut>(window: &WindowConfig, main: F)
    where
        F: FnOnce(MacroquadPlatform) -> Fut + 'static,
        Fut: Future<Output = ()> + 'static,
    {
        ::macroquad::Window::from_config(to_conf(window), async move {
            let backend = MacroquadPlatform {
                subscriber: register_input_subscriber(),
                focus: FocusTracker { focused: true },
            };
            main(backend).await;
        });
    }

    fn poll_focus(&mut self) {
        repeat_all_miniquad_input(&mut self.focus, self.subscriber);
    }
}

impl PlatformBackend for MacroquadPlatform {
    fn frame_time(&self) -> f64 {
        f64::from(::macroquad::time::get_frame_time())
    }

    fn next_frame(&mut self) -> FrameFuture<'_> {
        Box::pin(async move {
            ::macroquad::window::next_frame().await;
            self.poll_focus();
        })
    }

    fn is_key_down(&self, key: Key) -> bool {
        to_vendor(key).is_some_and(::macroquad::input::is_key_down)
    }

    fn keys_down(&self) -> Vec<Key> {
        let mut keys: Vec<Key> = ::macroquad::input::get_keys_down()
            .into_iter()
            .filter_map(from_vendor)
            .collect();
        keys.sort_unstable();
        keys
    }

    fn has_focus(&self) -> bool {
        self.focus.focused
    }

    fn window_size(&self) -> (u32, u32) {
        // Non-negative by construction; `as` saturates for out-of-range floats.
        (
            ::macroquad::window::screen_width().max(0.0) as u32,
            ::macroquad::window::screen_height().max(0.0) as u32,
        )
    }

    fn set_window_size(&mut self, width: u32, height: u32) {
        ::macroquad::window::request_new_screen_size(width as f32, height as f32);
    }

    fn set_fullscreen(&mut self, fullscreen: bool) {
        ::macroquad::window::set_fullscreen(fullscreen);
    }

    fn read_file(&self, path: &Path) -> Result<Vec<u8>> {
        // User files only (saves/config/logs), not the assets VFS (§13). Desktop-only for
        // v1: macroquad's own `load_file` is async (for wasm) and also reads through
        // `std::fs` on desktop.
        std::fs::read(path).map_err(|err| PlatformError::from_io(path, err))
    }

    fn write_file(&mut self, path: &Path, bytes: &[u8]) -> Result<()> {
        std::fs::write(path, bytes).map_err(|err| PlatformError::from_io(path, err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_key_maps_both_ways() {
        for &key in Key::ALL {
            let code = to_vendor(key).unwrap_or_else(|| panic!("{key:?} has no macroquad code"));
            assert_eq!(from_vendor(code), Some(key));
        }
        assert_eq!(KEY_MAP.len(), Key::ALL.len());
    }

    #[test]
    fn conf_mirrors_window_config() {
        let window = WindowConfig {
            title: "t".into(),
            width: 720,
            height: 480,
            fullscreen: true,
            resizable: false,
        };
        let conf = to_conf(&window);
        assert_eq!(conf.window_title, "t");
        assert_eq!((conf.window_width, conf.window_height), (720, 480));
        assert!(conf.fullscreen);
        assert!(!conf.window_resizable);
    }

    #[test]
    fn focus_tracker_follows_events() {
        let mut tracker = FocusTracker { focused: true };
        tracker.window_minimized_event();
        assert!(!tracker.focused);
        tracker.key_down_event(KeyCode::A, KeyMods::default(), false);
        assert!(tracker.focused);
        tracker.window_minimized_event();
        tracker.window_restored_event();
        assert!(tracker.focused);
    }
}
