//! `FileWatcher` port and adapters (§4.5, §8.3).

mod memory;
mod notify;
mod reload;

pub use memory::{MemoryFileWatcher, WatchSink};
pub use notify::NotifyWatcher;
pub use reload::{ReloadAction, ReloadKind, is_map_script, reload_actions, reload_kind};

/// Why a watcher could not start.
#[derive(Debug, thiserror::Error)]
pub enum WatchError {
    /// Root path problem.
    #[error("cannot watch `{path}`: {message}")]
    Io {
        /// Watched root.
        path: String,
        /// OS / `notify` detail.
        message: String,
    },
    /// Path outside the root or invalid.
    #[error("invalid watched path: {0}")]
    InvalidPath(String),
}

/// Watcher result type.
pub type Result<T> = std::result::Result<T, WatchError>;

/// Kind of change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchChange {
    /// Created or modified.
    Modify,
    /// Removed.
    Remove,
}

/// Normalised vfs-relative path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatchEvent {
    /// Project-relative path (`scripts/main.lua`).
    pub path: String,
    /// What happened.
    pub change: WatchChange,
}

/// Poll-based file change port.
pub trait FileWatcher: Send {
    /// Drain queued events.
    fn poll(&mut self) -> Vec<WatchEvent>;
}

/// Ignores changes.
#[derive(Clone, Debug, Default)]
pub struct NullFileWatcher;

impl FileWatcher for NullFileWatcher {
    fn poll(&mut self) -> Vec<WatchEvent> {
        Vec::new()
    }
}
