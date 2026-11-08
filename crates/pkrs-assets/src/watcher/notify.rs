//! `NotifyWatcher` for `pkrs run`.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use super::{FileWatcher, Result, WatchChange, WatchError, WatchEvent};
use crate::path::VfsPath;

/// Recursive project folder watcher.
pub struct NotifyWatcher {
    _watcher: RecommendedWatcher,
    rx: Receiver<notify::Result<Event>>,
    root: PathBuf,
}

impl NotifyWatcher {
    /// Watches `root` recursively.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let root = root.as_ref();
        let canonical = root.canonicalize().map_err(|e| WatchError::Io {
            path: root.display().to_string(),
            message: e.to_string(),
        })?;
        let (tx, rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                let _ = tx.send(res);
            },
            notify::Config::default(),
        )
        .map_err(|e| WatchError::Io {
            path: canonical.display().to_string(),
            message: e.to_string(),
        })?;
        watcher
            .watch(&canonical, RecursiveMode::Recursive)
            .map_err(|e| WatchError::Io {
                path: canonical.display().to_string(),
                message: e.to_string(),
            })?;
        Ok(Self {
            _watcher: watcher,
            rx,
            root: canonical,
        })
    }

    fn relativise(&self, path: &Path) -> Result<String> {
        let abs = path.canonicalize().map_err(|e| WatchError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        let rel = abs
            .strip_prefix(&self.root)
            .map_err(|_| WatchError::InvalidPath(abs.display().to_string()))?;
        let joined = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/");
        let vfs = VfsPath::new(&joined).map_err(|e| WatchError::InvalidPath(e.to_string()))?;
        Ok(vfs.to_string())
    }
}

impl FileWatcher for NotifyWatcher {
    fn poll(&mut self) -> Vec<WatchEvent> {
        let mut out = Vec::new();
        while let Ok(Ok(event)) = self.rx.try_recv() {
            let change = match event.kind {
                notify::EventKind::Create(_) | notify::EventKind::Modify(_) => WatchChange::Modify,
                notify::EventKind::Remove(_) => WatchChange::Remove,
                _ => continue,
            };
            for path in event.paths {
                if path.is_dir() {
                    continue;
                }
                match self.relativise(&path) {
                    Ok(vfs_path) => out.push(WatchEvent {
                        path: vfs_path,
                        change,
                    }),
                    Err(err) => tracing::debug!(%err, "notify: skip path"),
                }
            }
        }
        out
    }
}
