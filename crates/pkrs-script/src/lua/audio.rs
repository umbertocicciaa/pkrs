//! The `audio` module (§9.1, §11). Every call only queues a request; the engine's audio flush
//! runs it at the end of the tick (§3.3 step 6). None of them block.
//!
//! - `audio.play_bgm(id, opts?)`: `opts` is `{ fade = ticks }` or a tick count. The same id
//!   keeps playing; another id crossfades (30 ticks by default).
//! - `audio.stop_bgm(opts?)`: fades out (`{ fade = ticks }` or a tick count).
//! - `audio.play_sfx(id)`: fire and forget.
//! - `audio.play_jingle(id)` (deprecated alias `audio.jingle(id)`, API 1): ducks or pauses the BGM until it ends.
//! - `audio.set_volume(group, volume)`: `group` is `"master"`, `"bgm"`, `"sfx"` or
//!   `"jingle"`, `volume` a number in `0..=1`.

use mlua::{Lua, Table, Value};
use pkrs_audio::{AudioRequest, VolumeGroup};

use super::facade::{check, name, ticks, type_of, with_world};

fn send(lua: &Lua, func: &'static str, request: AudioRequest) -> mlua::Result<()> {
    let r = with_world(lua, func, |w| w.audio(request))?;
    check(lua, r.map_err(|e| format!("{func}: {e}")))
}

/// `nil`, a tick count or `{ fade = ticks }`.
fn fade(func: &str, v: &Value) -> Result<Option<u32>, String> {
    match v {
        Value::Nil => Ok(None),
        Value::Table(t) => {
            let fade: Value = t
                .raw_get("fade")
                .map_err(|e| format!("{func}: options: {e}"))?;
            if fade.is_nil() {
                Ok(None)
            } else {
                ticks(func, &fade, 0).map(Some)
            }
        }
        Value::Integer(_) | Value::Number(_) => ticks(func, v, 0).map(Some),
        other => Err(format!(
            "{func}: options must be nil, a tick count or {{ fade = ticks }} (got {})",
            type_of(other)
        )),
    }
}

fn group(v: &Value) -> Result<VolumeGroup, String> {
    const FUNC: &str = "audio.set_volume";
    let raw = name(FUNC, "group", v)?;
    VolumeGroup::parse(&raw).ok_or_else(|| {
        format!("{FUNC}: group must be \"master\", \"bgm\", \"sfx\" or \"jingle\" (got \"{raw}\")")
    })
}

fn volume(v: &Value) -> Result<f32, String> {
    const FUNC: &str = "audio.set_volume";
    let n = match v {
        Value::Integer(i) => *i as f64,
        Value::Number(n) => *n,
        other => {
            return Err(format!(
                "{FUNC}: volume must be a number (got {})",
                type_of(other)
            ));
        }
    };
    if !(0.0..=1.0).contains(&n) {
        return Err(format!("{FUNC}: volume must be between 0 and 1 (got {n})"));
    }
    Ok(n as f32)
}

/// Installs `audio` into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let audio = lua.create_table()?;
    audio.raw_set(
        "play_bgm",
        lua.create_function(|lua, (id, opts): (Value, Value)| {
            const FUNC: &str = "audio.play_bgm";
            let id = check(lua, name(FUNC, "id", &id))?;
            let fade = check(lua, fade(FUNC, &opts))?;
            send(lua, FUNC, AudioRequest::PlayBgm { id, fade })
        })?,
    )?;
    audio.raw_set(
        "stop_bgm",
        lua.create_function(|lua, opts: Value| {
            const FUNC: &str = "audio.stop_bgm";
            let fade = check(lua, fade(FUNC, &opts))?;
            send(lua, FUNC, AudioRequest::StopBgm { fade })
        })?,
    )?;
    audio.raw_set(
        "play_sfx",
        lua.create_function(|lua, id: Value| {
            const FUNC: &str = "audio.play_sfx";
            let id = check(lua, name(FUNC, "id", &id))?;
            send(lua, FUNC, AudioRequest::PlaySfx { id })
        })?,
    )?;
    let jingle = lua.create_function(|lua, id: Value| {
        const FUNC: &str = "audio.play_jingle";
        let id = check(lua, name(FUNC, "id", &id))?;
        send(lua, FUNC, AudioRequest::PlayJingle { id })
    })?;
    audio.raw_set("play_jingle", jingle.clone())?;
    audio.raw_set("jingle", jingle)?;
    audio.raw_set(
        "set_volume",
        lua.create_function(|lua, (g, v): (Value, Value)| {
            let group = check(lua, group(&g))?;
            let volume = check(lua, volume(&v))?;
            send(
                lua,
                "audio.set_volume",
                AudioRequest::SetVolume { group, volume },
            )
        })?,
    )?;
    env.raw_set("audio", audio)
}
