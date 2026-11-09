//! The `ScriptWorld` port: what the Lua API v0 (§9.1) may ask of the engine.
//!
//! Lua bindings never see runtime types. They call this engine-owned trait, expressed in engine
//! vocabulary only ([`GridPos`], [`Direction`], [`ScriptValue`]); the runtime implements it
//! over its world state, and tests implement it with fakes (§4.5).
//!
//! Requests that take time (`ui.say`, `ui.choose`, `npc:move`) only *start* here: the binding
//! then yields the task, and the engine wakes it with a
//! [`WakeSignal`](crate::WakeSignal) when the dialogue closes or the path ends (§9.3).

use core::fmt;

use pkrs_core::{Direction, GridPos};
use pkrs_data::{ENGINE_LANGUAGE, TextArgs, format_text};
use pkrs_render::UniformValue;

use crate::host::ScriptValue;

/// A request the world refused (unknown NPC, invalid item, …). The message is shown to the
/// script author, prefixed with the calling function (`npc:move: …`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApiError(String);

impl ApiError {
    /// Error with `message`.
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    /// The message.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ApiError {}

/// Result of a [`ScriptWorld`] request.
pub type ApiResult<T> = Result<T, ApiError>;

/// What `map.tile(x, y)` reports about one logical tile.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TileInfo {
    /// Fully blocked (collision layer or `solid = true`).
    pub solid: bool,
    /// Terrain name (`grass`, `water`, `ledge_s`, a custom terrain …), if any.
    pub terrain: Option<String>,
}

/// Who the camera should follow (`camera.follow`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CameraTarget {
    /// The player (default).
    Player,
    /// The NPC with this id.
    Npc(String),
}

/// A camera effect requested by a script (`camera.*`, §5.3, §9.1). None of them block; scripts
/// `wait(n)` for the duration they asked for.
#[derive(Clone, Debug, PartialEq)]
pub enum CameraCommand {
    /// Pan to the centre of tile `to` over `ticks` ticks (`0` = cut).
    PanTo {
        /// Target tile.
        to: GridPos,
        /// Duration in ticks.
        ticks: u32,
    },
    /// Follow a target again.
    Follow(CameraTarget),
    /// Shake by up to `intensity` pixels for `ticks` ticks.
    Shake {
        /// Maximum offset in pixels.
        intensity: u32,
        /// Duration in ticks.
        ticks: u32,
    },
    /// Fade the screen to black over `ticks` ticks.
    FadeOut {
        /// Duration in ticks.
        ticks: u32,
    },
    /// Fade back in from black over `ticks` ticks.
    FadeIn {
        /// Duration in ticks.
        ticks: u32,
    },
}

/// The engine services reachable from the Lua API v0 (`game`, `map`, `npc`, `player`, `ui`,
/// `camera`), installed with [`LuaScriptHost::set_world`](crate::LuaScriptHost::set_world).
///
/// Arguments are validated by the bindings before they reach the port (names are non-empty,
/// counts positive, directions parsed); implementations validate what only the world knows
/// (does the NPC exist, is the tile on the map) and answer with an [`ApiError`].
pub trait ScriptWorld {
    // --- game (§9.1, §6.5) ---

    /// `game.flag(name)`: `false` when never set.
    fn flag(&self, name: &str) -> bool;
    /// `game.set_flag(name, value)`.
    fn set_flag(&mut self, name: &str, value: bool);
    /// `game.var(name)`: [`ScriptValue::Nil`] when never set.
    fn var(&self, name: &str) -> ScriptValue;
    /// `game.set_var(name, value)`; `Nil` clears it.
    fn set_var(&mut self, name: &str, value: ScriptValue);
    /// `game.give_item(id, count)`; `count >= 1`.
    fn give_item(&mut self, item: &str, count: u32) -> ApiResult<()>;
    /// `game.bag()`: `(item, count)` pairs.
    fn bag(&self) -> Vec<(String, u32)>;
    /// `game.money()`.
    fn money(&self) -> i64;

    // --- map (§9.1, §6.4, §6.5) ---

    /// `map.current()`: id of the active map, if one is loaded.
    fn current_map(&self) -> Option<String>;
    /// `map.warp(map, x, y, facing)`: queues a warp; it happens after the scripts phase.
    fn warp(&mut self, map: &str, to: GridPos, facing: Option<Direction>) -> ApiResult<()>;
    /// `map.tile(x, y)`: `None` outside the map.
    fn tile(&self, pos: GridPos) -> Option<TileInfo>;
    /// `map.var(name)`: map-local variable of the active map (persisted, §6.5).
    fn map_var(&self, name: &str) -> ScriptValue;
    /// `map.set_var(name, value)`.
    fn set_map_var(&mut self, name: &str, value: ScriptValue);

    // --- npc (§9.1) ---

    /// `true` if the active map has an NPC `id` (`map.npc(id)`).
    fn npc_exists(&self, id: &str) -> bool;
    /// `npc:pos()`: tile and facing.
    fn npc_pos(&self, id: &str) -> ApiResult<(GridPos, Direction)>;
    /// `npc:face(dir)`.
    fn npc_face(&mut self, id: &str, dir: Direction) -> ApiResult<()>;
    /// `npc:move(path)`: starts walking `path` (one tile per entry). The engine reports the end
    /// with [`WakeSignal::PathFinished`](crate::WakeSignal::PathFinished).
    fn npc_move(&mut self, id: &str, path: &[Direction]) -> ApiResult<()>;
    /// `npc:set_sprite(sprite)`.
    fn npc_set_sprite(&mut self, id: &str, sprite: &str) -> ApiResult<()>;
    /// `npc:hide()` / `npc:show()`.
    fn npc_set_visible(&mut self, id: &str, visible: bool) -> ApiResult<()>;

    // --- player (§9.1) ---

    /// `player.pos()`: tile and facing.
    fn player_pos(&self) -> (GridPos, Direction);
    /// `player.face(dir)`.
    fn player_face(&mut self, dir: Direction);
    /// `player.lock()` / `player.unlock()`: the script-held lock, on top of the automatic one
    /// held by interact/trigger tasks (§9.3).
    fn set_player_locked(&mut self, locked: bool);
    /// `player.set_field_state(name, value)` (e.g. `surf`).
    fn set_field_state(&mut self, name: &str, value: bool) -> ApiResult<()>;

    // --- ui (§9.1, §5.5) ---

    /// `ui.say(text, args)`: opens a dialogue box. `text` is literal text or a `text:` key,
    /// localized by the engine with `args` filling its `{name}` / `{0}` placeholders (§5.5).
    /// The engine signals
    /// [`WakeSignal::DialogueClosed`](crate::WakeSignal::DialogueClosed) when it closes.
    fn say(&mut self, text: &str, args: &TextArgs) -> ApiResult<()>;
    /// `ui.choose(options)` / `ui.yes_no(text, args)`: opens a choice list (with an optional
    /// prompt and its arguments). Prompt and options are localized like [`say`](Self::say).
    /// The engine signals `DialogueClosed` with the 1-based index picked, or `Nil` on cancel.
    fn choose(
        &mut self,
        prompt: Option<&str>,
        args: &TextArgs,
        options: &[String],
    ) -> ApiResult<()>;
    /// `ui.notify(text, args)`: a non-blocking toast, localized like [`say`](Self::say).
    fn notify(&mut self, text: &str, args: &TextArgs);

    // --- localization (§5.5, §2.2 `[locale]`) ---

    /// `game.language()`: the active language. Default: [`ENGINE_LANGUAGE`].
    fn language(&self) -> String {
        ENGINE_LANGUAGE.to_owned()
    }
    /// `game.languages()`: the languages `game.set_language` accepts. Default: just
    /// [`language`](Self::language).
    fn languages(&self) -> Vec<String> {
        vec![self.language()]
    }
    /// `game.set_language(lang)`: switches every text shown from now on to `lang` (one of
    /// [`languages`](Self::languages)). Default: refused.
    fn set_language(&mut self, language: &str) -> ApiResult<()> {
        Err(ApiError::new(format!(
            "cannot switch to `{language}`: this world has no text tables"
        )))
    }
    /// `game.text(value, args)`: `value` localized in the active language. Default: `value`
    /// with its placeholders filled in.
    fn text(&self, value: &str, args: &TextArgs) -> String {
        format_text(value, args)
    }

    // --- camera (§9.1, §5.3) ---

    /// `camera.*`.
    fn camera(&mut self, command: CameraCommand) -> ApiResult<()>;

    // --- render (§9.1, §5.6) ---

    /// `render.set_global_uniform(name, value)`: sets a global material uniform (the name is
    /// non-empty and not an engine uniform). Default: refused.
    fn set_global_uniform(&mut self, name: &str, value: UniformValue) -> ApiResult<()> {
        let _ = value;
        Err(ApiError::new(format!(
            "cannot set `{name}`: this world has no renderer"
        )))
    }
}

/// Direction from `from` toward `to` along the dominant axis (ties prefer vertical); `None`
/// when both are the same tile. Used by `npc:face_player()`.
#[must_use]
pub fn direction_toward(from: GridPos, to: GridPos) -> Option<Direction> {
    let dx = i64::from(to.x) - i64::from(from.x);
    let dy = i64::from(to.y) - i64::from(from.y);
    if dx == 0 && dy == 0 {
        return None;
    }
    Some(if dy.abs() >= dx.abs() {
        if dy < 0 {
            Direction::Up
        } else {
            Direction::Down
        }
    } else if dx < 0 {
        Direction::Left
    } else {
        Direction::Right
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_toward_prefers_the_dominant_axis() {
        let o = GridPos::new(5, 5);
        assert_eq!(direction_toward(o, GridPos::new(5, 4)), Some(Direction::Up));
        assert_eq!(
            direction_toward(o, GridPos::new(5, 9)),
            Some(Direction::Down)
        );
        assert_eq!(
            direction_toward(o, GridPos::new(2, 6)),
            Some(Direction::Left)
        );
        assert_eq!(
            direction_toward(o, GridPos::new(8, 5)),
            Some(Direction::Right)
        );
        assert_eq!(
            direction_toward(o, GridPos::new(6, 6)),
            Some(Direction::Down)
        );
        assert_eq!(direction_toward(o, o), None);
    }

    #[test]
    fn api_error_displays_its_message() {
        assert_eq!(ApiError::new("no npc `x`").to_string(), "no npc `x`");
    }
}
