//! [`SaveFs`]: the file-system port save slots are written through (§12).
//!
//! [`StdFs`] is the real file system. Tests substitute a port that fails between the steps of
//! an atomic write to check that a crash never loses the previous save.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// The file operations an atomic save write needs.
pub trait SaveFs {
    /// Reads the whole file.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Creates (or truncates) `path`, writes `bytes` and flushes them to disk (`fsync`).
    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// Renames `from` over `to`, replacing `to` atomically.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Removes the file.
    fn remove(&self, path: &Path) -> io::Result<()>;

    /// Creates `dir` and its parents.
    fn create_dir_all(&self, dir: &Path) -> io::Result<()>;

    /// Flushes `dir`'s entries (the rename) to disk where the OS supports it.
    fn sync_dir(&self, _dir: &Path) -> io::Result<()> {
        Ok(())
    }
}

/// [`SaveFs`] on `std::fs`. `rename` replaces the target atomically on every supported OS (on
/// Windows through `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`).
#[derive(Clone, Copy, Debug, Default)]
pub struct StdFs;

impl SaveFs for StdFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        fs::read(path)
    }

    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let mut file = fs::File::create(path)?;
        file.write_all(bytes)?;
        file.sync_all()
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }

    fn create_dir_all(&self, dir: &Path) -> io::Result<()> {
        fs::create_dir_all(dir)
    }

    #[cfg(unix)]
    fn sync_dir(&self, dir: &Path) -> io::Result<()> {
        fs::File::open(dir)?.sync_all()
    }
}
