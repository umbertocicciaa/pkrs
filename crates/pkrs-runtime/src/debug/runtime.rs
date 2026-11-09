//! DevTools state wired into [`ProjectHost`](crate::project::ProjectHost).

use pkrs_assets::{FileWatcher, NullFileWatcher, WatchSink};
use std::collections::BTreeSet;

use pkrs_platform::{Key, Platform};
use pkrs_render::Renderer;
use pkrs_world::Map;

use super::collision::render_collision_view;
use super::console::DevConsole;
use super::overlay::{DebugOverlaySnapshot, render_overlay};
use super::reload::ReloadPlan;
use crate::project::ProjectHost;
use crate::ui_theme::UiTheme;

/// Dev-only overlay, watcher and console (§15.1).
pub struct DevRuntime {
    show_stats: bool,
    show_collision: bool,
    console: DevConsole,
    pending: ReloadPlan,
    watch: Box<dyn FileWatcher>,
    reload_error: Option<String>,
    smoothed_fps: f32,
    prev_keys: BTreeSet<Key>,
}

impl DevRuntime {
    /// Dev disabled (no watcher).
    pub fn disabled() -> Self {
        Self {
            show_stats: false,
            show_collision: false,
            console: DevConsole::new(),
            pending: ReloadPlan::empty(),
            watch: Box::new(NullFileWatcher),
            reload_error: None,
            smoothed_fps: 60.0,
            prev_keys: BTreeSet::new(),
        }
    }

    /// Dev with a file watcher.
    pub fn with_watcher(watcher: Box<dyn FileWatcher>) -> Self {
        Self {
            watch: watcher,
            ..Self::disabled()
        }
    }

    /// Test watcher backed by a shared queue.
    pub fn from_memory_sink(sink: WatchSink) -> Self {
        Self::with_watcher(Box::new(pkrs_assets::MemoryFileWatcher::new(sink)))
    }

    /// Watcher rooted at a project folder (debug builds).
    #[cfg(debug_assertions)]
    pub fn for_project(root: &std::path::Path) -> Self {
        match pkrs_assets::NotifyWatcher::new(root) {
            Ok(w) => Self::with_watcher(Box::new(w)),
            Err(err) => {
                tracing::warn!(%err, "dev file watcher disabled");
                Self::disabled()
            }
        }
    }

    /// No watcher in release builds.
    #[cfg(not(debug_assertions))]
    pub fn for_project(_root: &std::path::Path) -> Self {
        Self::disabled()
    }

    /// Hotkeys, file events and console input (each frame).
    pub fn poll(&mut self, platform: &Platform, host: &mut ProjectHost) {
        let keys: BTreeSet<Key> = platform.keys_down().into_iter().collect();
        let edge = |k: Key| keys.contains(&k) && !self.prev_keys.contains(&k);
        if edge(Key::F3) {
            self.show_stats = !self.show_stats;
        }
        if edge(Key::F4) {
            self.show_collision = !self.show_collision;
        }
        if edge(Key::F5) {
            self.pending.absorb(ReloadPlan::manual_scripts());
        }
        if edge(Key::F6) {
            self.pending.absorb(ReloadPlan::manual_data());
        }
        let dt = platform.raw_frame_time();
        if dt.is_finite() && dt > 0.0 {
            let fps = (1.0 / dt) as f32;
            self.smoothed_fps = self.smoothed_fps * 0.9 + fps * 0.1;
        }
        self.pending
            .absorb(ReloadPlan::from_events(&self.watch.poll()));
        if self.console.poll_keys(&keys, &self.prev_keys) {
            if let Some(scripts) = host.scripts_mut() {
                let line = self.console.line().to_owned();
                self.console.execute(scripts.host_mut(), &line);
            }
        }
        self.prev_keys = keys;
    }

    /// Runs queued reloads at a tick boundary.
    pub fn apply_pending(&mut self, host: &mut ProjectHost) {
        let plan = std::mem::replace(&mut self.pending, ReloadPlan::empty());
        if plan.is_empty() {
            return;
        }
        if plan.scripts {
            host.reload_scripts();
        }
        if plan.data {
            match host.reload_data() {
                Ok(()) => self.clear_reload_error(),
                Err(message) => self.set_reload_error(message),
            }
        }
        if plan.textures {
            let _ = host.reload_textures();
        }
    }

    /// Shows a data reload failure on the overlay.
    pub fn set_reload_error(&mut self, msg: impl Into<String>) {
        self.reload_error = Some(msg.into());
    }

    /// Clears the data reload error banner.
    pub fn clear_reload_error(&mut self) {
        self.reload_error = None;
    }

    #[must_use]
    /// Whether the F3 stats panel is visible.
    pub fn show_stats(&self) -> bool {
        self.show_stats
    }

    #[must_use]
    /// Whether the F4 collision view is visible.
    pub fn show_collision(&self) -> bool {
        self.show_collision
    }

    #[must_use]
    /// Exponential moving average of frame rate.
    pub fn smoothed_fps(&self) -> f32 {
        self.smoothed_fps
    }

    /// Draws dev overlays after the scene (host metadata is collected by the caller).
    pub fn render_to(
        &self,
        renderer: &mut Renderer,
        _platform: &Platform,
        theme: Option<&UiTheme>,
        map: &Map,
        stats: Option<&DebugOverlaySnapshot>,
        origin: (i32, i32),
    ) {
        if self.show_collision {
            render_collision_view(
                renderer,
                map,
                &map.grid,
                origin,
                (map.tile_width, map.tile_height),
            );
        }
        let Some(theme) = theme else {
            return;
        };
        if let Some(snap) = stats {
            render_overlay(renderer, theme, snap);
        }
        if let Some(msg) = &self.reload_error {
            renderer.draw_text(
                &theme.font,
                theme.font_tex,
                msg,
                4.0,
                20.0,
                pkrs_render::TextStyle {
                    tint: pkrs_render::Color {
                        r: 255,
                        g: 200,
                        b: 200,
                        a: 255,
                    },
                    scale: 1,
                    layer: pkrs_render::Layer::Ui,
                },
            );
        }
        if self.console.is_open() {
            let prompt = format!("> {}", self.console.line());
            renderer.draw_text(
                &theme.font,
                theme.font_tex,
                &prompt,
                4.0,
                renderer.view_size().1 as f32 * 0.55,
                pkrs_render::TextStyle {
                    tint: pkrs_render::Color::WHITE,
                    scale: 1,
                    layer: pkrs_render::Layer::Ui,
                },
            );
            if let Some(status) = self.console.status_text() {
                renderer.draw_text(
                    &theme.font,
                    theme.font_tex,
                    status,
                    4.0,
                    renderer.view_size().1 as f32 * 0.55 + 12.0,
                    pkrs_render::TextStyle {
                        tint: pkrs_render::Color::WHITE,
                        scale: 1,
                        layer: pkrs_render::Layer::Ui,
                    },
                );
            }
        }
    }
}
