//! `SubVfs`: one directory of another [`Vfs`] served as its own mount root.
//!
//! Plugin assets are mounted this way (§9.4, §13): `plugins/<id>/assets/` of the project
//! becomes the `<id>:` mount, so `resonance:ui/icon.png` reads
//! `plugins/resonance/assets/ui/icon.png`, whichever adapter backs the project.

use super::{Vfs, VfsEntry};
use crate::error::{AssetsError, Result};
use crate::path::VfsPath;

/// A [`Vfs`] restricted to directory `dir` of `inner`. Paths are normalised before they get
/// here, so they cannot climb out of `dir`.
#[derive(Clone, Debug)]
pub struct SubVfs<V> {
    inner: V,
    dir: VfsPath,
}

impl<V: Vfs> SubVfs<V> {
    /// Serves `dir` of `inner` as the root.
    pub fn new(inner: V, dir: VfsPath) -> Self {
        Self { inner, dir }
    }

    /// The directory of the inner file system this mount serves.
    pub fn dir(&self) -> &VfsPath {
        &self.dir
    }

    fn outer(&self, path: &VfsPath) -> Result<VfsPath> {
        if path.is_root() {
            Ok(self.dir.clone())
        } else if self.dir.is_root() {
            Ok(path.clone())
        } else {
            VfsPath::new(&format!("{}/{}", self.dir, path))
        }
    }

    fn inner_path(&self, outer: &VfsPath) -> Result<VfsPath> {
        if self.dir.is_root() {
            return Ok(outer.clone());
        }
        let rest = outer
            .as_str()
            .strip_prefix(self.dir.as_str())
            .and_then(|r| r.strip_prefix('/'))
            .ok_or_else(|| AssetsError::NotFound(outer.to_string()))?;
        VfsPath::new(rest)
    }
}

impl<V: Vfs> Vfs for SubVfs<V> {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> {
        self.inner.read(&self.outer(path)?)
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.outer(path).is_ok_and(|p| self.inner.exists(&p))
    }

    fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>> {
        self.inner
            .list(&self.outer(dir)?)?
            .into_iter()
            .map(|e| {
                Ok(VfsEntry {
                    path: self.inner_path(&e.path)?,
                    kind: e.kind,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vfs::{EntryKind, MemoryVfs};

    fn p(raw: &str) -> VfsPath {
        VfsPath::new(raw).unwrap()
    }

    #[test]
    fn serves_one_directory_as_root() {
        let vfs = MemoryVfs::new()
            .with("plugins/mud/assets/ui/icon.png", b"icon".to_vec())
            .unwrap()
            .with("plugins/mud/init.lua", b"--".to_vec())
            .unwrap();
        let sub = SubVfs::new(vfs, p("plugins/mud/assets"));
        assert_eq!(sub.read(&p("ui/icon.png")).unwrap(), b"icon");
        assert!(sub.exists(&p("ui/icon.png")));
        assert!(!sub.exists(&p("init.lua")));
        // Paths are normalised before they reach the adapter, so they cannot climb out.
        assert!(VfsPath::new("../init.lua").is_err());
        let root = sub.list(&VfsPath::root()).unwrap();
        assert_eq!(
            root,
            [VfsEntry {
                path: p("ui"),
                kind: EntryKind::Dir
            }]
        );
        assert_eq!(sub.list(&p("ui")).unwrap()[0].path, p("ui/icon.png"));
    }
}
