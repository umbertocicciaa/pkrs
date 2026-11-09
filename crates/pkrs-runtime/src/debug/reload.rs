//! Hot reload planning and queue (§8.3).

pub use pkrs_assets::watcher::reload_kind as classify_path;
use pkrs_assets::watcher::{FileWatcher, ReloadAction, WatchEvent, reload_actions};

/// Subsystems to refresh at the next tick boundary.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ReloadPlan {
    /// Lua scripts (game, map, plugins).
    pub scripts: bool,
    /// `data/` and locale tables.
    pub data: bool,
    /// Sprites, tilesets and the active map image.
    pub textures: bool,
}

impl ReloadPlan {
    /// Empty plan.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            scripts: false,
            data: false,
            textures: false,
        }
    }

    /// **F5** — reload scripts only.
    #[must_use]
    pub const fn manual_scripts() -> Self {
        Self {
            scripts: true,
            data: false,
            textures: false,
        }
    }

    /// **F6** — reload data only.
    #[must_use]
    pub const fn manual_data() -> Self {
        Self {
            scripts: false,
            data: true,
            textures: false,
        }
    }

    /// `true` when nothing is queued.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !self.scripts && !self.data && !self.textures
    }

    /// Merges watch events into a reload plan.
    #[must_use]
    pub fn from_events(events: &[WatchEvent]) -> Self {
        let actions = reload_actions(events);
        Self::from_actions(&actions)
    }

    /// Merges coalesced reload actions.
    #[must_use]
    pub fn from_actions(actions: &[ReloadAction]) -> Self {
        let mut plan = Self::empty();
        for action in actions {
            match action {
                ReloadAction::Scripts => plan.scripts = true,
                ReloadAction::Data => plan.data = true,
                ReloadAction::Textures => plan.textures = true,
            }
        }
        plan
    }

    /// Unions `other` into `self`.
    pub fn absorb(&mut self, other: Self) {
        self.scripts |= other.scripts;
        self.data |= other.data;
        self.textures |= other.textures;
    }
}

/// Queues reload actions from file events for tick-boundary drains (tests and helpers).
#[derive(Debug, Default)]
pub struct HotReload {
    pending: Vec<ReloadAction>,
    data_error: Option<String>,
}

impl HotReload {
    /// Empty queue.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Last data reload validation failure, if any.
    #[must_use]
    pub fn data_error(&self) -> Option<&str> {
        self.data_error.as_deref()
    }

    /// Clears the data error banner.
    pub fn clear_data_error(&mut self) {
        self.data_error = None;
    }

    /// Records a failed data reload (old data stays live).
    pub fn set_data_error(&mut self, message: impl Into<String>) {
        self.data_error = Some(message.into());
    }

    /// Pulls changes from `watcher` and merges them into `pending`.
    pub fn poll_watcher(&mut self, watcher: &mut dyn FileWatcher) {
        let events = watcher.poll();
        if events.is_empty() {
            return;
        }
        for action in reload_actions(&events) {
            if !self.pending.contains(&action) {
                self.pending.push(action);
            }
        }
        tracing::info!(target: "dev", changed = events.len(), "hot reload queued");
    }

    /// Queues a manual reload (F5/F6).
    pub fn queue(&mut self, action: ReloadAction) {
        if !self.pending.contains(&action) {
            self.pending.push(action);
        }
    }

    /// Takes queued actions in submission order (one pass per tick).
    pub fn drain_at_tick(&mut self) -> Vec<ReloadAction> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_assets::watcher::{MemoryFileWatcher, ReloadKind, WatchChange, WatchEvent, WatchSink};
    use std::sync::{Arc, Mutex};

    #[test]
    fn classify_paths_for_reload() {
        assert_eq!(classify_path("scripts/maps/town.lua"), ReloadKind::Scripts);
        assert_eq!(classify_path("saves/slot1.sav"), ReloadKind::Ignore);
    }

    #[test]
    fn watcher_events_queue_actions() {
        let sink: WatchSink = Arc::new(Mutex::new(Vec::new()));
        sink.lock().unwrap().push(WatchEvent {
            path: "scripts/maps/town.lua".into(),
            change: WatchChange::Modify,
        });
        let mut hot = HotReload::new();
        let mut w = MemoryFileWatcher::new(sink);
        hot.poll_watcher(&mut w);
        assert_eq!(hot.drain_at_tick().len(), 1);
    }
}
