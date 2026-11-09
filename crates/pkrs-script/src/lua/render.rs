//! The `render` module (§9.1, §5.6): scripts set material parameters, never GPU code.
//!
//! - `render.set_global_uniform(name, value)`: sets the global uniform `name` read by
//!   `Global(name: …)` material uniforms. `value` is a number (`float`) or an array of 2, 3
//!   or 4 numbers (`vec2`, `vec3`, `vec4`; colours are `{ r, g, b, a }` in 0–1). Engine
//!   uniforms (`time_ticks`, `map_origin`, `texture_size`) cannot be set. Plugins should
//!   namespace their globals (`daynight:tint`).

use mlua::{Lua, Table, Value};
use pkrs_render::{UniformValue, is_engine_uniform};

use super::facade::{check, name, type_of, with_world};

const FUNC: &str = "render.set_global_uniform";

/// A Lua value as a uniform value.
fn uniform_value(v: &Value) -> Result<UniformValue, String> {
    let number = |v: &Value| -> Option<f32> {
        match v {
            Value::Integer(i) => Some(*i as f32),
            Value::Number(n) if n.is_finite() => Some(*n as f32),
            _ => None,
        }
    };
    if let Some(n) = number(v) {
        return Ok(UniformValue::Float(n));
    }
    let Value::Table(t) = v else {
        return Err(format!(
            "{FUNC}: value must be a number or an array of 2-4 numbers (got {})",
            type_of(v)
        ));
    };
    let len = t.raw_len();
    if !(2..=4).contains(&len) {
        return Err(format!(
            "{FUNC}: value must be an array of 2-4 numbers (got {len} entries)"
        ));
    }
    let mut out = [0.0f32; 4];
    for (i, slot) in out.iter_mut().enumerate().take(len) {
        let entry: Value = t.raw_get(i + 1).map_err(|e| format!("{FUNC}: {e}"))?;
        *slot = number(&entry).ok_or_else(|| {
            format!(
                "{FUNC}: value[{}] must be a finite number (got {})",
                i + 1,
                type_of(&entry)
            )
        })?;
    }
    Ok(match len {
        2 => UniformValue::Vec2([out[0], out[1]]),
        3 => UniformValue::Vec3([out[0], out[1], out[2]]),
        _ => UniformValue::Vec4(out),
    })
}

/// Installs `render` into `env`.
pub(super) fn install(lua: &Lua, env: &Table) -> mlua::Result<()> {
    let render = lua.create_table()?;
    render.raw_set(
        "set_global_uniform",
        lua.create_function(|lua, (n, v): (Value, Value)| {
            let uniform = check(lua, name(FUNC, "name", &n))?;
            if is_engine_uniform(&uniform) {
                return check(
                    lua,
                    Err(format!(
                        "{FUNC}: `{uniform}` is an engine uniform and cannot be set"
                    )),
                );
            }
            let value = check(lua, uniform_value(&v))?;
            let r = with_world(lua, FUNC, |w| w.set_global_uniform(&uniform, value))?;
            check(lua, r.map_err(|e| format!("{FUNC}: {e}")))
        })?,
    )?;
    env.raw_set("render", render)
}
