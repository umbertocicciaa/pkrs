//! `MemoryFileWatcher` for headless reload tests.

use std::sync::{Arc, Mutex};

use super::{FileWatcher, WatchChange, WatchEvent};

/// Queue drained by a [`MemoryFileWatcher`]. Tests push [`WatchEvent`]s; [`FileWatcher::poll`] reads them.
pub type WatchSink = Arc<Mutex<Vec<WatchEvent>>>;

/// Drains `sink`.
#[derive(Clone, Debug)]
pub struct MemoryFileWatcher {
    sink: WatchSink,
}

impl MemoryFileWatcher {
    /// Watcher for `sink`.
    #[must_use]
    pub fn new(sink: WatchSink) -> Self {
        Self { sink }
    }

    /// Queues a modify (tests).
    pub fn push_modify(&self, path: impl Into<String>) {
        if let Ok(mut q) = self.sink.lock() {
            q.push(WatchEvent {
                path: path.into(),
                change: WatchChange::Modify,
            });
        }
    }
}

impl FileWatcher for MemoryFileWatcher {
    fn poll(&mut self) -> Vec<WatchEvent> {
        self.sink
            .lock()
            .map_or(Vec::new(), |mut q| std::mem::take(&mut *q))
    }
}
