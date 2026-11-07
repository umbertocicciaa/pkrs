//! The `camera` module (§9.1, §5.3). None of these block; a cutscene waits for the effect with
//! `wait(n)`.
//!
//! - `camera.pan_to(x, y, ticks = 0)`: pan to the centre of tile `x, y`.
//! - `camera.follow(target = "player")`: `"player"`, an `npc` object or an NPC id.
//! - `camera.shake(intensity = 2, ticks = 20)`: intensity in pixels (0–64).
//! - `camera.fade_out(ticks = 20)` / `camera.fade_in(ticks = 20)`.

use mlua::{Lua, Table, Value};

use super::facade::{check, int, pos, ticks, type_of, with_world};
use super::npc;
use crate::world::{CameraCommand, CameraTarget};

/// Default length of a fade or shake, in ticks.
const DEFAULT_EFFECT_TICKS: u32 = 20;

fn send(lua: &Lua, func: &'static str, command: CameraCommand) -> mlua::Result<()> {
    let r = with_world(lua, func, |w| w.camera(command))?;
    check(lua, r.map_err(|e| format!("{func}: {e}")))
}

fn target(v: &Value) -> Result<CameraTarget, String> {
    match v {
        Value::Nil => Ok(CameraTarget::Player),
        Value::String(s) => {
            let s = s.to_string_lossy();
            match s.as_str() {
                "player" => Ok(CameraTarget::Player),
                "" => Err("camera.follow: target must not be empty".into()),
                _ => Ok(CameraTarget::Npc(s)),
            }
        }
        Value::Table(t) => npc::handle_id(t).map(CameraTarget::Npc).ok_or_else(|| {
            "camera.follow: target must be \"player\", an npc object or an npc id".into()
        }),
        other => Err(format!(
            "camera.follow: target must be \"player\", an npc object or an npc id (got {})",
            type_of(other)
        )),
    }
}

/// Installs `camera` into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let camera = lua.create_table()?;
    camera.raw_set(
        "pan_to",
        lua.create_function(|lua, (x, y, n): (Value, Value, Value)| {
            const FUNC: &str = "camera.pan_to";
            let to = check(lua, pos(FUNC, &x, &y))?;
            let ticks = check(lua, ticks(FUNC, &n, 0))?;
            send(lua, FUNC, CameraCommand::PanTo { to, ticks })
        })?,
    )?;
    camera.raw_set(
        "follow",
        lua.create_function(|lua, who: Value| {
            let who = check(lua, target(&who))?;
            send(lua, "camera.follow", CameraCommand::Follow(who))
        })?,
    )?;
    camera.raw_set(
        "shake",
        lua.create_function(|lua, (intensity, n): (Value, Value)| {
            const FUNC: &str = "camera.shake";
            let intensity = if intensity.is_nil() {
                2
            } else {
                check(lua, int(FUNC, "intensity", &intensity, 0, 64))?
            };
            let ticks = check(lua, ticks(FUNC, &n, DEFAULT_EFFECT_TICKS))?;
            let intensity = u32::try_from(intensity).unwrap_or(0);
            send(lua, FUNC, CameraCommand::Shake { intensity, ticks })
        })?,
    )?;
    camera.raw_set(
        "fade_out",
        lua.create_function(|lua, n: Value| {
            let ticks = check(lua, ticks("camera.fade_out", &n, DEFAULT_EFFECT_TICKS))?;
            send(lua, "camera.fade_out", CameraCommand::FadeOut { ticks })
        })?,
    )?;
    camera.raw_set(
        "fade_in",
        lua.create_function(|lua, n: Value| {
            let ticks = check(lua, ticks("camera.fade_in", &n, DEFAULT_EFFECT_TICKS))?;
            send(lua, "camera.fade_in", CameraCommand::FadeIn { ticks })
        })?,
    )?;
    env.raw_set("camera", camera)
}
