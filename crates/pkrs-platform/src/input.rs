//! Input facade: actions, per-tick edges, movement buffer and menu repeat (§10).

use std::collections::BTreeSet;

use pkrs_core::Direction;
use pkrs_core::manifest::InputConfig as ManifestInputConfig;

use crate::action::{Action, ActionRegistry, BuiltinAction, CustomActionId, resolve_action};
use crate::error::{InputError, InputResult};
use crate::key::Key;
use crate::platform::Platform;

/// Policy for menu cursor key repeat while a direction is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuRepeatConfig {
    /// Ticks to wait after the initial press before the first repeat.
    pub initial_delay_ticks: u32,
    /// Ticks between further repeats while held.
    pub repeat_rate_ticks: u32,
}

impl Default for MenuRepeatConfig {
    fn default() -> Self {
        Self {
            initial_delay_ticks: 20,
            repeat_rate_ticks: 4,
        }
    }
}

/// Policy for overworld movement direction buffering at the end of a step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MovementBufferConfig {
    /// Buffer direction presses during the last N ticks of the current step.
    pub window_ticks: u32,
}

impl Default for MovementBufferConfig {
    fn default() -> Self {
        Self { window_ticks: 8 }
    }
}

/// Engine policy applied by the [`Input`] facade.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputFacadeConfig {
    /// Menu cursor repeat while a direction is held.
    pub menu_repeat: MenuRepeatConfig,
    /// Overworld direction buffering near the end of a step.
    pub movement_buffer: MovementBufferConfig,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ActionTickState {
    down: bool,
    prev_down: bool,
    held_ticks: u32,
    pressed: bool,
    released: bool,
}

#[derive(Clone, Debug)]
struct ActionBinding {
    keys: Vec<Key>,
    state: ActionTickState,
}

/// Maps platform keys to actions with per-tick edge detection and buffering.
pub struct Input {
    config: InputFacadeConfig,
    registry: ActionRegistry,
    builtins: [ActionBinding; BuiltinAction::ALL.len()],
    custom: Vec<ActionBinding>,
    keys_down: BTreeSet<Key>,
    press_carry: BTreeSet<Action>,
    movement_buffer: Option<Direction>,
    step_ticks_remaining: u32,
}

impl Default for Input {
    fn default() -> Self {
        Self::new(InputFacadeConfig::default())
    }
}

impl Input {
    /// Creates an input facade with built-in default bindings (§10).
    pub fn new(config: InputFacadeConfig) -> Self {
        let mut input = Self {
            config,
            registry: ActionRegistry::default(),
            builtins: std::array::from_fn(|_| ActionBinding {
                keys: Vec::new(),
                state: ActionTickState::default(),
            }),
            custom: Vec::new(),
            keys_down: BTreeSet::new(),
            press_carry: BTreeSet::new(),
            movement_buffer: None,
            step_ticks_remaining: 0,
        };
        input.apply_default_bindings();
        input
    }

    /// Builds bindings from a validated project manifest `[input]` table, falling back to
    /// built-in defaults for actions not listed.
    pub fn from_manifest(
        config: InputFacadeConfig,
        manifest: &ManifestInputConfig,
    ) -> InputResult<Self> {
        let mut input = Self::new(config);
        input.apply_manifest(manifest)?;
        Ok(input)
    }

    /// Registers a plugin action and binds `default_keys`.
    pub fn register_action(&mut self, name: &str, default_keys: &[Key]) -> InputResult<Action> {
        let id = self.registry.register(name, default_keys)?;
        self.custom.push(ActionBinding {
            keys: default_keys.to_vec(),
            state: ActionTickState::default(),
        });
        Ok(Action::Custom(id))
    }

    /// Resolves an action name (`confirm`, `resonance:toggle`, …).
    pub fn action(&self, name: &str) -> Option<Action> {
        resolve_action(name, &self.registry)
    }

    /// Polls keyboard state once per rendered frame (§3.1).
    pub fn poll(&mut self, platform: &Platform) {
        self.keys_down.clear();
        if platform.is_paused() {
            return;
        }
        for key in platform.keys_down() {
            self.keys_down.insert(key);
        }
    }

    /// Recomputes per-tick edges; call once at the start of each simulation tick.
    pub fn begin_tick(&mut self) {
        self.update_binding(&Action::Builtin(BuiltinAction::Up));
        self.update_binding(&Action::Builtin(BuiltinAction::Down));
        self.update_binding(&Action::Builtin(BuiltinAction::Left));
        self.update_binding(&Action::Builtin(BuiltinAction::Right));
        self.update_binding(&Action::Builtin(BuiltinAction::Confirm));
        self.update_binding(&Action::Builtin(BuiltinAction::Cancel));
        self.update_binding(&Action::Builtin(BuiltinAction::Menu));
        self.update_binding(&Action::Builtin(BuiltinAction::Run));
        self.update_binding(&Action::Builtin(BuiltinAction::Select));
        for index in 0..self.custom.len() {
            let id = CustomActionId::new(index as u32);
            self.update_binding(&Action::Custom(id));
        }

        self.capture_movement_buffer();
    }

    /// Whether `action` is held this tick.
    pub fn down(&self, action: Action) -> bool {
        self.binding(action).state.down
    }

    /// Whether `action` had a press edge this simulation tick.
    pub fn pressed(&self, action: Action) -> bool {
        self.binding(action).state.pressed
    }

    /// Whether `action` has a press waiting from an earlier tick (§10 menu buffer).
    pub fn pending(&self, action: Action) -> bool {
        self.press_carry.contains(&action)
    }

    /// Press edge this tick or a carried press not yet [`consume`](Self::consume)d.
    pub fn triggered(&self, action: Action) -> bool {
        self.pressed(action) || self.pending(action)
    }

    /// Whether `action` was released this tick.
    pub fn released(&self, action: Action) -> bool {
        self.binding(action).state.released
    }

    /// Consecutive ticks `action` has been held (resets to `0` on release).
    pub fn held_ticks(&self, action: Action) -> u32 {
        self.binding(action).state.held_ticks
    }

    /// Marks `action` as handled so it is not carried to a later state/tick.
    pub fn consume(&mut self, action: Action) {
        self.press_carry.remove(&action);
    }

    /// Updates how many ticks remain in the current overworld step (for movement buffering).
    pub fn set_movement_step_remaining(&mut self, ticks_remaining: u32) {
        self.step_ticks_remaining = ticks_remaining;
    }

    /// Takes a direction buffered during the last [`MovementBufferConfig::window_ticks`] of the step.
    pub fn take_movement_direction(&mut self) -> Option<Direction> {
        self.movement_buffer.take()
    }

    /// Returns a menu cursor direction when it should move this tick (initial press or repeat).
    pub fn menu_direction(&self) -> Option<Direction> {
        let delay = self.config.menu_repeat.initial_delay_ticks;
        let rate = self.config.menu_repeat.repeat_rate_ticks;
        for (action, direction) in MENU_DIRECTIONS {
            let held = self.held_ticks(Action::Builtin(action));
            if held == 0 {
                continue;
            }
            if menu_repeat_fires(held, delay, rate) {
                return Some(direction);
            }
        }
        None
    }

    fn apply_default_bindings(&mut self) {
        for (action, keys) in DEFAULT_BINDINGS {
            let binding = self.builtin_binding_mut(*action);
            binding.keys = keys.to_vec();
        }
    }

    fn apply_manifest(&mut self, manifest: &ManifestInputConfig) -> InputResult<()> {
        for (name, key_names) in &manifest.bindings {
            if key_names.is_empty() {
                return Err(InputError::EmptyBinding(name.clone()));
            }
            let keys = parse_key_names(name, key_names)?;
            if let Some(builtin) = BuiltinAction::parse(name) {
                self.builtin_binding_mut(builtin).keys = keys;
            } else if let Some(id) = self.registry.id_by_name(name) {
                self.custom[id.get() as usize].keys = keys;
            } else {
                let id = self.registry.register(name, &keys)?;
                self.custom.push(ActionBinding {
                    keys,
                    state: ActionTickState::default(),
                });
                let _ = id;
            }
        }
        Ok(())
    }

    fn update_binding(&mut self, action: &Action) {
        let keys = self.binding(*action).keys.clone();
        let raw_down = keys.iter().any(|key| self.keys_down.contains(key));
        let binding = self.binding_mut(*action);
        let prev = binding.state.prev_down;
        binding.state.down = raw_down;
        binding.state.pressed = raw_down && !prev;
        binding.state.released = !raw_down && prev;
        binding.state.held_ticks = if raw_down {
            binding.state.held_ticks.saturating_add(1)
        } else {
            0
        };
        binding.state.prev_down = raw_down;
        if binding.state.pressed {
            self.press_carry.insert(*action);
        }
    }

    fn capture_movement_buffer(&mut self) {
        if self.step_ticks_remaining > self.config.movement_buffer.window_ticks {
            return;
        }
        for (action, direction) in MENU_DIRECTIONS {
            if self.down(Action::Builtin(action)) {
                self.movement_buffer = Some(direction);
            }
        }
    }

    fn binding(&self, action: Action) -> &ActionBinding {
        match action {
            Action::Builtin(b) => &self.builtins[b.index()],
            Action::Custom(id) => &self.custom[id.get() as usize],
        }
    }

    fn binding_mut(&mut self, action: Action) -> &mut ActionBinding {
        match action {
            Action::Builtin(b) => &mut self.builtins[b.index()],
            Action::Custom(id) => &mut self.custom[id.get() as usize],
        }
    }

    fn builtin_binding_mut(&mut self, action: BuiltinAction) -> &mut ActionBinding {
        &mut self.builtins[action.index()]
    }
}

const MENU_DIRECTIONS: [(BuiltinAction, Direction); 4] = [
    (BuiltinAction::Up, Direction::Up),
    (BuiltinAction::Down, Direction::Down),
    (BuiltinAction::Left, Direction::Left),
    (BuiltinAction::Right, Direction::Right),
];

const DEFAULT_BINDINGS: &[(BuiltinAction, &[Key])] = &[
    (BuiltinAction::Up, &[Key::Up, Key::W]),
    (BuiltinAction::Down, &[Key::Down, Key::S]),
    (BuiltinAction::Left, &[Key::Left, Key::A]),
    (BuiltinAction::Right, &[Key::Right, Key::D]),
    (BuiltinAction::Confirm, &[Key::Z, Key::Enter]),
    (BuiltinAction::Cancel, &[Key::X, Key::Backspace]),
    (BuiltinAction::Menu, &[Key::Escape]),
    (BuiltinAction::Run, &[Key::LeftShift]),
    (BuiltinAction::Select, &[Key::Space]),
];

fn parse_key_names(_action: &str, names: &[String]) -> InputResult<Vec<Key>> {
    let mut keys = Vec::with_capacity(names.len());
    for name in names {
        let key = Key::parse_name(name).ok_or_else(|| InputError::UnknownKeyName(name.clone()))?;
        keys.push(key);
    }
    Ok(keys)
}

fn menu_repeat_fires(held_ticks: u32, initial_delay: u32, repeat_rate: u32) -> bool {
    if held_ticks == 0 {
        return false;
    }
    if held_ticks == 1 {
        return true;
    }
    if held_ticks <= initial_delay {
        return false;
    }
    let rate = repeat_rate.max(1);
    (held_ticks - initial_delay - 1) % rate == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixedPaths, NullFrame, NullPlatform, Platform, PlatformConfig, block_on};

    fn platform_with(frames: impl IntoIterator<Item = NullFrame>) -> Platform {
        Platform::new(
            Box::new(NullPlatform::new().with_frames(frames)),
            Box::new(FixedPaths::under("input-test")),
            PlatformConfig::default(),
        )
    }

    fn tick(input: &mut Input, platform: &Platform) {
        input.poll(platform);
        input.begin_tick();
    }

    #[test]
    fn default_bindings_match_design_fixture() {
        let mut input = Input::default();
        let mut platform = platform_with([NullFrame::with_keys([Key::Z])]);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.pressed(Action::Builtin(BuiltinAction::Confirm)));
        assert!(input.down(Action::Builtin(BuiltinAction::Confirm)));
    }

    #[test]
    fn multi_tick_frame_counts_one_press() {
        let mut input = Input::default();
        let mut platform = platform_with([NullFrame {
            dt: 0.25,
            keys: vec![Key::Z],
            ..NullFrame::idle()
        }]);
        block_on(platform.next_frame());
        input.poll(&platform);
        let mut presses = 0u32;
        for _ in 0..15 {
            input.begin_tick();
            if input.pressed(Action::Builtin(BuiltinAction::Confirm)) {
                presses += 1;
            }
        }
        assert_eq!(presses, 1);
        assert_eq!(
            input.held_ticks(Action::Builtin(BuiltinAction::Confirm)),
            15
        );
    }

    #[test]
    fn zero_tick_frames_latch_press_until_next_tick() {
        let mut input = Input::default();
        let step = 1.0 / 144.0;
        let mut platform = platform_with([
            NullFrame {
                dt: step,
                keys: vec![Key::Z],
                ..NullFrame::idle()
            },
            NullFrame {
                dt: 1.0 / 60.0,
                keys: vec![Key::Z],
                ..NullFrame::idle()
            },
        ]);
        block_on(platform.next_frame());
        input.poll(&platform);

        block_on(platform.next_frame());
        input.poll(&platform);
        input.begin_tick();
        assert!(input.pressed(Action::Builtin(BuiltinAction::Confirm)));
    }

    #[test]
    fn held_ticks_reset_on_release() {
        let mut input = Input::default();
        let mut platform = platform_with([
            NullFrame::with_keys([Key::Z]),
            NullFrame::with_keys([Key::Z]),
            NullFrame::idle(),
        ]);
        for _ in 0..2 {
            block_on(platform.next_frame());
            tick(&mut input, &platform);
        }
        assert_eq!(input.held_ticks(Action::Builtin(BuiltinAction::Confirm)), 2);

        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert_eq!(input.held_ticks(Action::Builtin(BuiltinAction::Confirm)), 0);
        assert!(input.released(Action::Builtin(BuiltinAction::Confirm)));
    }

    #[test]
    fn menu_repeat_timing() {
        let config = InputFacadeConfig {
            menu_repeat: MenuRepeatConfig {
                initial_delay_ticks: 3,
                repeat_rate_ticks: 2,
            },
            ..InputFacadeConfig::default()
        };
        let mut input = Input::new(config);
        let mut platform =
            platform_with(std::iter::repeat_n(NullFrame::with_keys([Key::Down]), 20));
        let mut fires = Vec::new();
        for tick_index in 1..=10 {
            block_on(platform.next_frame());
            tick(&mut input, &platform);
            if input.menu_direction() == Some(Direction::Down) {
                fires.push(tick_index);
            }
        }
        assert_eq!(fires, vec![1, 4, 6, 8, 10]);
    }

    #[test]
    fn movement_buffer_only_in_window() {
        let config = InputFacadeConfig {
            movement_buffer: MovementBufferConfig { window_ticks: 2 },
            ..InputFacadeConfig::default()
        };
        let mut input = Input::new(config);
        let mut platform = platform_with([
            NullFrame::with_keys([Key::Right]),
            NullFrame::with_keys([Key::Right]),
        ]);
        input.set_movement_step_remaining(5);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.take_movement_direction().is_none());

        input.set_movement_step_remaining(2);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert_eq!(input.take_movement_direction(), Some(Direction::Right));
    }

    #[test]
    fn press_carry_survives_until_consumed() {
        let mut input = Input::default();
        let mut platform = platform_with([NullFrame::with_keys([Key::Z])]);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.pressed(Action::Builtin(BuiltinAction::Confirm)));
        assert!(input.pending(Action::Builtin(BuiltinAction::Confirm)));

        input.consume(Action::Builtin(BuiltinAction::Confirm));
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(!input.triggered(Action::Builtin(BuiltinAction::Confirm)));
    }

    #[test]
    fn manifest_overrides_defaults() {
        let mut manifest = ManifestInputConfig::default();
        manifest
            .bindings
            .insert("confirm".to_owned(), vec!["space".to_owned()]);
        let input = Input::from_manifest(InputFacadeConfig::default(), &manifest).expect("valid");
        let mut platform = platform_with([NullFrame::with_keys([Key::Space])]);
        block_on(platform.next_frame());
        let mut input = input;
        tick(&mut input, &platform);
        assert!(input.pressed(Action::Builtin(BuiltinAction::Confirm)));
    }
}
