//! F3 stats overlay (§15.1).

use pkrs_core::GridPos;
use pkrs_render::{Color, Layer, Rect, Renderer, TextStyle};
use pkrs_script::{LuaScriptHost, ScriptHost};

use crate::ui_theme::UiTheme;

/// Plain data for the F3 panel (unit-tested without a renderer).
#[derive(Clone, Debug, PartialEq)]
pub struct DebugOverlaySnapshot {
    /// Frames per second from the last frame.
    pub fps: f32,
    /// Last frame time in milliseconds.
    pub frame_ms: f32,
    /// Simulation ticks elapsed.
    pub ticks: u64,
    /// Human-readable mode (`overworld`, `battle`, …).
    pub mode: String,
    /// Active map id.
    pub map: String,
    /// Player tile.
    pub player_tile: GridPos,
    /// Lua heap bytes.
    pub lua_bytes: usize,
    /// Suspended script tasks.
    pub script_tasks: usize,
}

/// Renders the F3 panel when `theme` is available.
pub fn render_overlay(renderer: &mut Renderer, theme: &UiTheme, snap: &DebugOverlaySnapshot) {
    let (vw, _) = renderer.view_size();
    let pad = 4.0;
    let line_h = 10.0;
    let lines = [
        format!("FPS {:.0}  {:.1} ms", snap.fps, snap.frame_ms),
        format!("tick {}  {}", snap.ticks, snap.mode),
        format!(
            "map {}  @ {},{}",
            snap.map, snap.player_tile.x, snap.player_tile.y
        ),
        format!(
            "Lua {} KiB  tasks {}",
            snap.lua_bytes / 1024,
            snap.script_tasks
        ),
    ];
    let box_h = pad * 2.0 + line_h * lines.len() as f32;
    renderer.draw_filled_rect(
        Rect::new(pad, pad, vw as f32 - pad * 2.0, box_h),
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 180,
        },
        Layer::Ui,
    );
    let style = TextStyle {
        tint: Color {
            r: 220,
            g: 255,
            b: 220,
            a: 255,
        },
        scale: 1,
        layer: Layer::Ui,
    };
    for (i, line) in lines.iter().enumerate() {
        renderer.draw_text(
            &theme.font,
            theme.font_tex,
            line,
            pad + 4.0,
            pad + 4.0 + line_h * i as f32,
            style,
        );
    }
}

/// Lua bytes and task count when scripting is enabled.
#[must_use]
pub fn script_overlay_fields(host: Option<&LuaScriptHost>, tasks: usize) -> (usize, usize) {
    match host {
        Some(h) => (h.memory_used(), tasks),
        None => (0, tasks),
    }
}
