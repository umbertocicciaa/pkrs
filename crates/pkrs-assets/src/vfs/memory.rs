//! `MemoryVfs`: in-memory files for tests and headless runs (§4.5 Null adapters).

use std::collections::{BTreeMap, BTreeSet};

use super::{EntryKind, Vfs, VfsEntry};
use crate::error::{AssetsError, Result};
use crate::path::VfsPath;

/// An in-memory [`Vfs`]. Directories are implicit: they exist when a file lives below them.
#[derive(Clone, Debug, Default)]
pub struct MemoryVfs {
    files: BTreeMap<VfsPath, Vec<u8>>,
}

impl MemoryVfs {
    /// Creates an empty file system.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds or replaces a file. The path is normalised; the root itself cannot be a file.
    pub fn insert(&mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Result<()> {
        let path = VfsPath::new(path)?;
        if path.is_root() {
            return Err(AssetsError::InvalidPath {
                path: String::new(),
                reason: "a file needs a name".to_owned(),
            });
        }
        self.files.insert(path, bytes.into());
        Ok(())
    }

    /// Builder form of [`insert`](Self::insert).
    pub fn with(mut self, path: &str, bytes: impl Into<Vec<u8>>) -> Result<Self> {
        self.insert(path, bytes)?;
        Ok(self)
    }

    /// Removes a file, returning its bytes.
    pub fn remove(&mut self, path: &VfsPath) -> Option<Vec<u8>> {
        self.files.remove(path)
    }

    /// Number of files.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Returns `true` if there are no files.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    fn case_insensitive_match(&self, path: &VfsPath) -> Option<&VfsPath> {
        let wanted = path.as_str().to_lowercase();
        self.files
            .keys()
            .find(|p| p.as_str().to_lowercase() == wanted)
    }
}

impl Vfs for MemoryVfs {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> {
        if let Some(bytes) = self.files.get(path) {
            return Ok(bytes.clone());
        }
        match self.case_insensitive_match(path) {
            Some(actual) => Err(AssetsError::CaseMismatch {
                requested: path.to_string(),
                actual: actual.to_string(),
            }),
            None => Err(AssetsError::NotFound(path.to_string())),
        }
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.files.contains_key(path)
    }

    fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>> {
        let depth = dir.segments().count();
        let mut entries = BTreeSet::new();
        for path in self.files.keys().filter(|p| p.starts_with_dir(dir)) {
            let segments: Vec<&str> = path.segments().collect();
            let Some(&name) = segments.get(depth) else {
                continue;
            };
            let kind = if segments.len() == depth + 1 {
                EntryKind::File
            } else {
                EntryKind::Dir
            };
            entries.insert(VfsEntry {
                path: dir.join_name(name)?,
                kind,
            });
        }
        if entries.is_empty() && !dir.is_root() {
            return Err(AssetsError::NotFound(dir.to_string()));
        }
        Ok(entries.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(raw: &str) -> VfsPath {
        VfsPath::new(raw).expect("valid")
    }

    fn sample() -> MemoryVfs {
        MemoryVfs::new()
            .with("sprites/hero.png", b"hero".to_vec())
            .and_then(|v| v.with("sprites/npc/old_man.png", b"old".to_vec()))
            .and_then(|v| v.with("pkrs.toml", b"[game]".to_vec()))
            .expect("valid")
    }

    #[test]
    fn read_and_exists() {
        let vfs = sample();
        assert_eq!(vfs.read(&p("sprites/hero.png")).expect("present"), b"hero");
        assert!(vfs.exists(&p("sprites/hero.png")));
        assert!(!vfs.exists(&p("sprites")), "directories are not files");
        assert!(matches!(
            vfs.read(&p("sprites/missing.png")),
            Err(AssetsError::NotFound(_))
        ));
        assert_eq!(vfs.len(), 3);
    }

    #[test]
    fn insert_normalises_paths() {
        let mut vfs = MemoryVfs::new();
        vfs.insert(".\\maps\\..\\maps\\town.tmx", b"<map/>".to_vec())
            .expect("valid");
        assert!(vfs.exists(&p("maps/town.tmx")));
        assert!(vfs.insert("../x", Vec::new()).is_err());
        assert!(vfs.insert("", Vec::new()).is_err());
    }

    #[test]
    fn case_mismatch_is_reported() {
        let vfs = sample();
        match vfs.read(&p("Sprites/Hero.png")) {
            Err(AssetsError::CaseMismatch { actual, .. }) => assert_eq!(actual, "sprites/hero.png"),
            other => panic!("expected CaseMismatch, got {other:?}"),
        }
    }

    #[test]
    fn list_returns_direct_children_sorted() {
        let vfs = sample();
        let root = vfs.list(&VfsPath::root()).expect("root");
        assert_eq!(
            root,
            [
                VfsEntry {
                    path: p("pkrs.toml"),
                    kind: EntryKind::File
                },
                VfsEntry {
                    path: p("sprites"),
                    kind: EntryKind::Dir
                },
            ]
        );
        let sprites = vfs.list(&p("sprites")).expect("dir");
        assert_eq!(
            sprites,
            [
                VfsEntry {
                    path: p("sprites/hero.png"),
                    kind: EntryKind::File
                },
                VfsEntry {
                    path: p("sprites/npc"),
                    kind: EntryKind::Dir
                },
            ]
        );
        assert!(matches!(
            vfs.list(&p("audio")),
            Err(AssetsError::NotFound(_))
        ));
        assert!(
            MemoryVfs::new()
                .list(&VfsPath::root())
                .expect("root")
                .is_empty()
        );
    }
}
