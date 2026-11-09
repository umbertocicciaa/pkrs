//! Input facade: actions, per-tick edges, movement buffer and menu repeat (§10).

use std::collections::{BTreeMap, BTreeSet};

use pkrs_core::Direction;
use pkrs_core::InputBuiltinPressed;
use pkrs_core::manifest::{InputConfig as ManifestInputConfig, is_action_name};

use crate::action::{Action, ActionRegistry, BuiltinAction, CustomActionId, resolve_action};
use crate::button::GamepadButton;
use crate::config::{UserBinding, UserConfig};
use crate::error::{InputError, InputResult};
use crate::key::Key;
use crate::platform::Platform;

/// One physical control that became newly held this tick (rebinding capture, §10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlSource {
    /// A keyboard key.
    Key(Key),
    /// A standard gamepad button, including a digital stick direction.
    Button(GamepadButton),
}

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

/// Vertical menu cursor movement produced by [`Input::menu_cursor_direction`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuCursorDirection {
    /// Cursor moves to the previous row.
    Up,
    /// Cursor moves to the next row.
    Down,
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
    buttons: Vec<GamepadButton>,
    state: ActionTickState,
}

/// Carried presses expire after this many simulation ticks (§10 menu buffer).
const PRESS_CARRY_TTL_TICKS: u64 = 8;

/// Maps platform keys to actions with per-tick edge detection and buffering.
pub struct Input {
    config: InputFacadeConfig,
    registry: ActionRegistry,
    builtins: [ActionBinding; BuiltinAction::ALL.len()],
    custom: Vec<ActionBinding>,
    keys_down: BTreeSet<Key>,
    keys_latched: BTreeSet<Key>,
    buttons_down: BTreeSet<GamepadButton>,
    buttons_latched: BTreeSet<GamepadButton>,
    prev_keys: BTreeSet<Key>,
    prev_buttons: BTreeSet<GamepadButton>,
    fresh: Vec<ControlSource>,
    press_carry: BTreeMap<Action, u64>,
    pending_overrides: BTreeMap<String, Vec<Key>>,
    pending_buttons: BTreeMap<String, Vec<GamepadButton>>,
    movement_buffer: Option<Direction>,
    step_ticks_remaining: u32,
    simulation_tick: u64,
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
                buttons: Vec::new(),
                state: ActionTickState::default(),
            }),
            custom: Vec::new(),
            keys_down: BTreeSet::new(),
            keys_latched: BTreeSet::new(),
            buttons_down: BTreeSet::new(),
            buttons_latched: BTreeSet::new(),
            prev_keys: BTreeSet::new(),
            prev_buttons: BTreeSet::new(),
            fresh: Vec::new(),
            press_carry: BTreeMap::new(),
            pending_overrides: BTreeMap::new(),
            pending_buttons: BTreeMap::new(),
            movement_buffer: None,
            step_ticks_remaining: 0,
            simulation_tick: 0,
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
        let keys = self
            .pending_overrides
            .remove(name)
            .unwrap_or_else(|| default_keys.to_vec());
        let buttons = self.pending_buttons.remove(name).unwrap_or_default();
        let id = self.registry.register(name)?;
        self.custom.push(ActionBinding {
            keys,
            buttons,
            state: ActionTickState::default(),
        });
        Ok(Action::Custom(id))
    }

    /// Resolves an action name (`confirm`, `resonance:toggle`, …).
    pub fn action(&self, name: &str) -> Option<Action> {
        resolve_action(name, &self.registry)
    }

    /// Keys currently bound to `action` (scripted input picks the first, §15.2).
    pub fn bound_keys(&self, action: Action) -> &[Key] {
        &self.binding(action).keys
    }

    /// Gamepad buttons currently bound to `action`.
    pub fn bound_buttons(&self, action: Action) -> &[GamepadButton] {
        &self.binding(action).buttons
    }

    /// Replaces bindings listed in `config`. Unlisted actions keep their current keys and
    /// buttons (built-in defaults, then the manifest). A listed action replaces both lists,
    /// so an empty button list clears the pad binding for that action.
    pub fn apply_user_config(&mut self, config: &UserConfig) -> InputResult<()> {
        let mut parsed = Vec::with_capacity(config.bindings.len());
        for (name, binding) in &config.bindings {
            if binding.keys.is_empty() && binding.buttons.is_empty() {
                return Err(InputError::EmptyBinding(name.clone()));
            }
            let keys = parse_key_names(name, &binding.keys)?;
            let buttons = parse_button_names(name, &binding.buttons)?;
            parsed.push((name.clone(), keys, buttons));
        }
        for (name, keys, buttons) in parsed {
            if let Some(builtin) = BuiltinAction::parse(&name) {
                let binding = self.builtin_binding_mut(builtin);
                binding.keys = keys;
                binding.buttons = buttons;
            } else if let Some(id) = self.registry.id_by_name(&name) {
                let binding = &mut self.custom[id.get() as usize];
                binding.keys = keys;
                binding.buttons = buttons;
            } else if name.contains(':') {
                if !is_action_name(&name) {
                    return Err(InputError::InvalidActionName(name));
                }
                self.pending_overrides.insert(name.clone(), keys);
                self.pending_buttons.insert(name, buttons);
            } else {
                return Err(InputError::InvalidActionName(name));
            }
        }
        Ok(())
    }

    /// Every current binding, built-ins first then custom actions, in config names.
    #[must_use]
    pub fn export_bindings(&self) -> BTreeMap<String, UserBinding> {
        let mut map = BTreeMap::new();
        for action in BuiltinAction::ALL {
            map.insert(
                action.name().to_owned(),
                self.user_binding(Action::Builtin(*action)),
            );
        }
        for (index, name) in self.registry.names().iter().enumerate() {
            let id = CustomActionId::new(index as u32);
            map.insert(name.clone(), self.user_binding(Action::Custom(id)));
        }
        map
    }

    fn user_binding(&self, action: Action) -> UserBinding {
        let binding = self.binding(action);
        UserBinding {
            keys: binding
                .keys
                .iter()
                .map(|key| key.config_name().to_owned())
                .collect(),
            buttons: binding
                .buttons
                .iter()
                .map(|button| button.name().to_owned())
                .collect(),
        }
    }

    /// Names of every action: the built-ins, then registered custom actions in order.
    pub fn action_names(&self) -> Vec<String> {
        BuiltinAction::ALL
            .iter()
            .map(|b| b.name().to_owned())
            .chain(self.registry.names().iter().cloned())
            .collect()
    }

    /// Polls keyboard state once per rendered frame (§3.1). Gamepad buttons stay empty.
    pub fn poll(&mut self, platform: &Platform) {
        self.poll_with(platform, &[]);
    }

    /// Polls the keyboard and the buttons held on the active pad into one action state (§10).
    pub fn poll_with(&mut self, platform: &Platform, buttons: &[GamepadButton]) {
        self.keys_down.clear();
        self.buttons_down.clear();
        if platform.is_paused() {
            return;
        }
        for key in platform.keys_down() {
            self.keys_down.insert(key);
        }
        self.keys_latched.extend(self.keys_down.iter().copied());
        for button in buttons {
            self.buttons_down.insert(*button);
        }
        self.buttons_latched
            .extend(self.buttons_down.iter().copied());
    }

    /// Keys and buttons that became held since the previous tick (rebinding capture).
    #[must_use]
    pub fn fresh_sources(&self) -> &[ControlSource] {
        &self.fresh
    }

    /// Recomputes per-tick edges; call once at the start of each simulation tick.
    pub fn begin_tick(&mut self) {
        self.simulation_tick = self.simulation_tick.saturating_add(1);
        self.expire_press_carry();

        let keys_latched = self.keys_latched.clone();
        let buttons_latched = self.buttons_latched.clone();
        self.fresh.clear();
        for key in &keys_latched {
            if !self.prev_keys.contains(key) {
                self.fresh.push(ControlSource::Key(*key));
            }
        }
        for button in &buttons_latched {
            if !self.prev_buttons.contains(button) {
                self.fresh.push(ControlSource::Button(*button));
            }
        }
        for &builtin in BuiltinAction::ALL {
            self.update_binding(Action::Builtin(builtin), &keys_latched, &buttons_latched);
        }
        for index in 0..self.custom.len() {
            let id = CustomActionId::new(index as u32);
            self.update_binding(Action::Custom(id), &keys_latched, &buttons_latched);
        }

        self.capture_movement_buffer();
        self.prev_keys.clone_from(&self.keys_down);
        self.prev_buttons.clone_from(&self.buttons_down);
        self.keys_latched.clone_from(&self.keys_down);
        self.buttons_latched.clone_from(&self.buttons_down);
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
        self.press_carry.contains_key(&action)
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
        self.menu_cursor_direction_vdir()
    }

    /// Returns vertical menu cursor movement (up/down with repeat). Left/right are ignored.
    pub fn menu_cursor_direction(&self) -> Option<MenuCursorDirection> {
        self.menu_cursor_direction_vdir().and_then(|dir| match dir {
            Direction::Up => Some(MenuCursorDirection::Up),
            Direction::Down => Some(MenuCursorDirection::Down),
            _ => None,
        })
    }

    fn menu_cursor_direction_vdir(&self) -> Option<Direction> {
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
            self.builtin_binding_mut(*action).keys = keys.to_vec();
        }
        for (action, buttons) in DEFAULT_PAD_BINDINGS {
            self.builtin_binding_mut(*action).buttons = buttons.to_vec();
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
            } else if name.contains(':') {
                if !is_action_name(name) {
                    return Err(InputError::InvalidActionName(name.clone()));
                }
                self.pending_overrides.insert(name.clone(), keys);
            } else {
                return Err(InputError::InvalidActionName(name.clone()));
            }
        }
        Ok(())
    }

    fn update_binding(
        &mut self,
        action: Action,
        keys_latched: &BTreeSet<Key>,
        buttons_latched: &BTreeSet<GamepadButton>,
    ) {
        let binding_keys = self.binding(action).keys.clone();
        let binding_buttons = self.binding(action).buttons.clone();
        let down = binding_keys.iter().any(|key| self.keys_down.contains(key))
            || binding_buttons
                .iter()
                .any(|button| self.buttons_down.contains(button));
        let latched = binding_keys.iter().any(|key| keys_latched.contains(key))
            || binding_buttons
                .iter()
                .any(|button| buttons_latched.contains(button));
        let binding = self.binding_mut(action);
        let prev = binding.state.prev_down;
        binding.state.down = down;
        binding.state.pressed = latched && !prev;
        binding.state.released = prev && !down;
        binding.state.held_ticks = if down {
            binding.state.held_ticks.saturating_add(1)
        } else {
            0
        };
        binding.state.prev_down = down;
        if binding.state.pressed {
            self.press_carry.insert(action, self.simulation_tick);
        }
    }

    fn expire_press_carry(&mut self) {
        let tick = self.simulation_tick;
        self.press_carry
            .retain(|_, stored| tick.saturating_sub(*stored) <= PRESS_CARRY_TTL_TICKS);
    }

    fn capture_movement_buffer(&mut self) {
        let window = self.config.movement_buffer.window_ticks;
        if self.step_ticks_remaining == 0 || self.step_ticks_remaining > window {
            self.movement_buffer = None;
            return;
        }
        for (action, direction) in MENU_DIRECTIONS {
            if self.pressed(Action::Builtin(action)) {
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

impl InputBuiltinPressed for Input {
    fn builtin_pressed(&self, action: BuiltinAction) -> bool {
        self.pressed(Action::Builtin(action))
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

const DEFAULT_PAD_BINDINGS: &[(BuiltinAction, &[GamepadButton])] = &[
    (
        BuiltinAction::Up,
        &[GamepadButton::DPadUp, GamepadButton::LeftStickUp],
    ),
    (
        BuiltinAction::Down,
        &[GamepadButton::DPadDown, GamepadButton::LeftStickDown],
    ),
    (
        BuiltinAction::Left,
        &[GamepadButton::DPadLeft, GamepadButton::LeftStickLeft],
    ),
    (
        BuiltinAction::Right,
        &[GamepadButton::DPadRight, GamepadButton::LeftStickRight],
    ),
    (BuiltinAction::Confirm, &[GamepadButton::South]),
    (BuiltinAction::Cancel, &[GamepadButton::East]),
    (BuiltinAction::Menu, &[GamepadButton::Start]),
    (BuiltinAction::Run, &[GamepadButton::RightShoulder]),
    (BuiltinAction::Select, &[GamepadButton::Select]),
];

fn parse_button_names(action: &str, names: &[String]) -> InputResult<Vec<GamepadButton>> {
    let mut buttons = Vec::with_capacity(names.len());
    for name in names {
        let button =
            GamepadButton::parse_name(name).ok_or_else(|| InputError::UnknownButtonName {
                action: action.to_owned(),
                button: name.clone(),
            })?;
        buttons.push(button);
    }
    Ok(buttons)
}

fn parse_key_names(action: &str, names: &[String]) -> InputResult<Vec<Key>> {
    let mut keys = Vec::with_capacity(names.len());
    for name in names {
        let key = Key::parse_name(name).ok_or_else(|| InputError::UnknownKeyName {
            action: action.to_owned(),
            key: name.clone(),
        })?;
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
                dt: step,
                keys: vec![],
                ..NullFrame::idle()
            },
            NullFrame {
                dt: 1.0 / 60.0,
                keys: vec![],
                ..NullFrame::idle()
            },
        ]);
        block_on(platform.next_frame());
        input.poll(&platform);

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
            NullFrame::idle(),
            NullFrame::with_keys([Key::Right]),
        ]);
        input.set_movement_step_remaining(5);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.take_movement_direction().is_none());

        block_on(platform.next_frame());
        tick(&mut input, &platform);

        input.set_movement_step_remaining(2);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert_eq!(input.take_movement_direction(), Some(Direction::Right));
    }

    #[test]
    fn movement_buffer_clears_when_idle() {
        let mut input = Input::default();
        let mut platform = platform_with([NullFrame::with_keys([Key::Up])]);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.take_movement_direction().is_none());

        block_on(platform.next_frame());
        tick(&mut input, &platform);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.take_movement_direction().is_none());
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
    fn press_carry_expires_after_ttl() {
        let mut input = Input::default();
        let mut platform = platform_with([NullFrame::with_keys([Key::X])]);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.pending(Action::Builtin(BuiltinAction::Cancel)));

        platform = platform_with(std::iter::repeat_n(NullFrame::idle(), 20));
        for _ in 0..20 {
            block_on(platform.next_frame());
            tick(&mut input, &platform);
        }
        assert!(!input.triggered(Action::Builtin(BuiltinAction::Cancel)));
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

    #[test]
    fn manifest_plugin_override_defers_until_register() {
        let mut manifest = ManifestInputConfig::default();
        manifest
            .bindings
            .insert("resonance:toggle".to_owned(), vec!["t".to_owned()]);
        let mut input =
            Input::from_manifest(InputFacadeConfig::default(), &manifest).expect("valid");
        let action = input
            .register_action("resonance:toggle", &[Key::R])
            .expect("register");
        let mut platform = platform_with([NullFrame::with_keys([Key::T])]);
        block_on(platform.next_frame());
        tick(&mut input, &platform);
        assert!(input.pressed(action));
    }

    #[test]
    fn manifest_rejects_unknown_builtin_typo() {
        let mut manifest = ManifestInputConfig::default();
        manifest
            .bindings
            .insert("confrim".to_owned(), vec!["z".to_owned()]);
        assert!(Input::from_manifest(InputFacadeConfig::default(), &manifest).is_err());
    }

    #[test]
    fn bound_keys_and_action_names_list_bindings() {
        let mut input = Input::default();
        assert_eq!(
            input.bound_keys(Action::Builtin(BuiltinAction::Confirm)),
            &[Key::Z, Key::Enter]
        );
        let toggle = input
            .register_action("resonance:toggle", &[Key::R])
            .expect("register");
        assert_eq!(input.bound_keys(toggle), &[Key::R]);
        let names = input.action_names();
        assert_eq!(names.len(), BuiltinAction::ALL.len() + 1);
        assert!(names.iter().any(|n| n == "confirm"));
        assert_eq!(names.last().map(String::as_str), Some("resonance:toggle"));
    }

    #[test]
    fn gamepad_button_merges_into_the_same_action() {
        let mut input = Input::default();
        let mut platform = platform_with([NullFrame::idle()]);
        block_on(platform.next_frame());
        input.poll_with(&platform, &[GamepadButton::DPadRight, GamepadButton::South]);
        input.begin_tick();
        assert!(input.pressed(Action::Builtin(BuiltinAction::Right)));
        assert!(input.down(Action::Builtin(BuiltinAction::Right)));
        assert!(input.pressed(Action::Builtin(BuiltinAction::Confirm)));
        assert!(!input.down(Action::Builtin(BuiltinAction::Left)));
    }

    #[test]
    fn user_config_overrides_manifest_and_survives_a_fresh_input() {
        let mut manifest = ManifestInputConfig::default();
        manifest
            .bindings
            .insert("confirm".to_owned(), vec!["space".to_owned()]);
        let mut config = UserConfig::default();
        config.bindings.insert(
            "confirm".to_owned(),
            UserBinding {
                keys: vec!["t".to_owned()],
                buttons: vec![],
            },
        );
        let mut input =
            Input::from_manifest(InputFacadeConfig::default(), &manifest).expect("manifest");
        input.apply_user_config(&config).expect("user");
        assert_eq!(
            input.bound_keys(Action::Builtin(BuiltinAction::Confirm)),
            &[Key::T]
        );
        assert!(
            input
                .bound_buttons(Action::Builtin(BuiltinAction::Confirm))
                .is_empty()
        );

        let saved = config.to_ron().expect("ron");
        let loaded = UserConfig::parse(&saved).expect("parse");
        let mut restarted =
            Input::from_manifest(InputFacadeConfig::default(), &manifest).expect("manifest");
        restarted.apply_user_config(&loaded).expect("user");
        assert_eq!(
            restarted.bound_keys(Action::Builtin(BuiltinAction::Confirm)),
            &[Key::T]
        );

        let mut platform = platform_with([NullFrame::with_keys([Key::Space])]);
        block_on(platform.next_frame());
        tick(&mut restarted, &platform);
        assert!(!restarted.pressed(Action::Builtin(BuiltinAction::Confirm)));

        let mut platform = platform_with([NullFrame::with_keys([Key::T])]);
        block_on(platform.next_frame());
        tick(&mut restarted, &platform);
        assert!(restarted.pressed(Action::Builtin(BuiltinAction::Confirm)));

        let mut platform = platform_with([NullFrame::idle()]);
        block_on(platform.next_frame());
        restarted.poll_with(&platform, &[GamepadButton::South]);
        restarted.begin_tick();
        assert!(!restarted.pressed(Action::Builtin(BuiltinAction::Confirm)));
    }
}
