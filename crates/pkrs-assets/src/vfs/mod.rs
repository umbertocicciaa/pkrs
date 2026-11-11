//! The `Vfs` port (§4.5, §13) and its in-tree adapters.
//!
//! A `Vfs` serves one mount root. It only ever sees normalised [`VfsPath`]s, so path policy
//! (separators, `..`, forbidden characters) lives in one place; adapters still guard against
//! escaping their root through the storage itself (e.g. symlinks).

mod folder;
mod memory;
mod pak;
mod sub;

pub use folder::FolderVfs;
pub use memory::MemoryVfs;
pub use pak::{PakBuilder, PakVfs};
pub use sub::SubVfs;

use crate::error::Result;
use crate::path::VfsPath;

/// Kind of a directory entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EntryKind {
    /// A directory.
    Dir,
    /// A regular file.
    File,
}

/// One entry returned by [`Vfs::list`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsEntry {
    /// Full path of the entry (relative to the mount root).
    pub path: VfsPath,
    /// File or directory.
    pub kind: EntryKind,
}

/// Engine-owned read-only file system port: loose folder, `.pak` archive or memory.
pub trait Vfs {
    /// Reads the whole file at `path`.
    ///
    /// Errors: [`NotFound`](crate::AssetsError::NotFound) if absent or a directory,
    /// [`CaseMismatch`](crate::AssetsError::CaseMismatch) if only the letter case differs,
    /// [`Io`](crate::AssetsError::Io) for storage failures.
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>>;

    /// Returns `true` if a file (not a directory) exists at exactly `path`.
    fn exists(&self, path: &VfsPath) -> bool;

    /// Lists the direct children of directory `dir`, sorted by path.
    ///
    /// Listing a missing directory is [`NotFound`](crate::AssetsError::NotFound).
    fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>>;
}

impl<V: Vfs + ?Sized> Vfs for Box<V> {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> {
        (**self).read(path)
    }

    fn exists(&self, path: &VfsPath) -> bool {
        (**self).exists(path)
    }

    fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>> {
        (**self).list(dir)
    }
}
