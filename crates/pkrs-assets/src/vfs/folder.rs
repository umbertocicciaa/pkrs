//! `FolderVfs`: loose project files on disk, used by `pkrs run` (§13).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::{EntryKind, Vfs, VfsEntry};
use crate::error::{AssetsError, Result};
use crate::path::VfsPath;

/// A [`Vfs`] over a directory on disk.
///
/// Every lookup walks the path segment by segment and requires the **exact** on-disk letter
/// case, so behaviour matches case-sensitive filesystems everywhere (see [`crate::path`]).
/// After resolution the real path must still be inside the root, which rejects symlinks that
/// point outside the project.
#[derive(Clone, Debug)]
pub struct FolderVfs {
    root: PathBuf,
}

impl FolderVfs {
    /// Opens `root`, which must be an existing directory.
    pub fn new(root: impl AsRef<Path>) -> Result<Self> {
        let raw = root.as_ref();
        let root = raw.canonicalize().map_err(|e| AssetsError::Io {
            path: raw.display().to_string(),
            message: e.to_string(),
        })?;
        if !root.is_dir() {
            return Err(AssetsError::Io {
                path: raw.display().to_string(),
                message: "not a directory".to_owned(),
            });
        }
        Ok(Self { root })
    }

    /// The canonical root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Maps `path` to a real path with exact case, inside the root.
    fn resolve(&self, path: &VfsPath) -> Result<PathBuf> {
        let mut current = self.root.clone();
        let mut actual: Vec<String> = Vec::new();
        let mut case_mismatch = false;
        for segment in path.segments() {
            let entries = fs::read_dir(&current).map_err(|e| io_error(path, &e))?;
            let mut exact = false;
            let mut folded = None;
            for entry in entries {
                let entry = entry.map_err(|e| io_error(path, &e))?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if name == segment {
                    exact = true;
                    break;
                }
                if folded.is_none() && name.to_lowercase() == segment.to_lowercase() {
                    folded = Some(name.to_owned());
                }
            }
            let name = if exact {
                segment.to_owned()
            } else if let Some(name) = folded {
                case_mismatch = true;
                name
            } else {
                return Err(AssetsError::NotFound(path.to_string()));
            };
            current.push(&name);
            actual.push(name);
        }
        if case_mismatch {
            return Err(AssetsError::CaseMismatch {
                requested: path.to_string(),
                actual: actual.join("/"),
            });
        }
        let real = current.canonicalize().map_err(|e| io_error(path, &e))?;
        if !real.starts_with(&self.root) {
            return Err(AssetsError::PathEscapesRoot(path.to_string()));
        }
        Ok(real)
    }
}

fn io_error(path: &VfsPath, err: &io::Error) -> AssetsError {
    match err.kind() {
        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory => {
            AssetsError::NotFound(path.to_string())
        }
        _ => AssetsError::Io {
            path: path.to_string(),
            message: err.to_string(),
        },
    }
}

impl Vfs for FolderVfs {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> {
        let real = self.resolve(path)?;
        if real.is_dir() {
            return Err(AssetsError::NotFound(path.to_string()));
        }
        fs::read(&real).map_err(|e| io_error(path, &e))
    }

    fn exists(&self, path: &VfsPath) -> bool {
        !path.is_root() && self.resolve(path).is_ok_and(|real| real.is_file())
    }

    fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>> {
        let real = self.resolve(dir)?;
        if !real.is_dir() {
            return Err(AssetsError::NotFound(dir.to_string()));
        }
        let mut entries = Vec::new();
        for entry in fs::read_dir(&real).map_err(|e| io_error(dir, &e))? {
            let entry = entry.map_err(|e| io_error(dir, &e))?;
            let name = entry.file_name();
            // Names that are not UTF-8 or not valid VFS segments cannot be addressed; skip them.
            let Some(path) = name.to_str().and_then(|n| dir.join_name(n).ok()) else {
                continue;
            };
            // Follows symlinks; dangling links are skipped.
            let Ok(meta) = fs::metadata(entry.path()) else {
                continue;
            };
            let kind = if meta.is_dir() {
                EntryKind::Dir
            } else {
                EntryKind::File
            };
            entries.push(VfsEntry { path, kind });
        }
        entries.sort();
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temporary project directory, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("pkrs-assets-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).expect("create temp dir");
            Self(dir)
        }

        fn file(&self, rel: &str, bytes: &[u8]) {
            let path = self.0.join(rel);
            fs::create_dir_all(path.parent().expect("has parent")).expect("mkdir");
            fs::write(path, bytes).expect("write");
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn p(raw: &str) -> VfsPath {
        VfsPath::new(raw).expect("valid")
    }

    #[test]
    fn reads_exists_and_lists() {
        let tmp = TempDir::new("folder-basic");
        tmp.file("project/sprites/hero.png", b"hero");
        tmp.file("project/pkrs.toml", b"[game]");
        let vfs = FolderVfs::new(tmp.0.join("project")).expect("open");

        assert_eq!(vfs.read(&p("sprites/hero.png")).expect("read"), b"hero");
        assert!(vfs.exists(&p("pkrs.toml")));
        assert!(!vfs.exists(&p("sprites")));
        assert!(!vfs.exists(&VfsPath::root()));
        assert!(matches!(
            vfs.read(&p("sprites")),
            Err(AssetsError::NotFound(_))
        ));
        assert!(matches!(
            vfs.read(&p("nope.png")),
            Err(AssetsError::NotFound(_))
        ));
        assert!(matches!(
            vfs.read(&p("pkrs.toml/child")),
            Err(AssetsError::NotFound(_))
        ));

        let root = vfs.list(&VfsPath::root()).expect("list");
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
        assert!(matches!(
            vfs.list(&p("audio")),
            Err(AssetsError::NotFound(_))
        ));
        assert!(matches!(
            vfs.list(&p("pkrs.toml")),
            Err(AssetsError::NotFound(_))
        ));
    }

    #[test]
    fn exact_case_is_required_on_every_os() {
        let tmp = TempDir::new("folder-case");
        tmp.file("Sprites/Hero.png", b"hero");
        let vfs = FolderVfs::new(&tmp.0).expect("open");
        assert!(vfs.exists(&p("Sprites/Hero.png")));
        assert!(!vfs.exists(&p("sprites/hero.png")));
        match vfs.read(&p("sprites/hero.png")) {
            Err(AssetsError::CaseMismatch { requested, actual }) => {
                assert_eq!(requested, "sprites/hero.png");
                assert_eq!(actual, "Sprites/Hero.png");
            }
            other => panic!("expected CaseMismatch, got {other:?}"),
        }
    }

    #[test]
    fn sibling_outside_the_root_is_unreachable() {
        let tmp = TempDir::new("folder-escape");
        tmp.file("secret.txt", b"secret");
        tmp.file("project/pkrs.toml", b"");
        let vfs = FolderVfs::new(tmp.0.join("project")).expect("open");
        // `..` cannot even be expressed as a VfsPath…
        assert!(matches!(
            VfsPath::new("../secret.txt"),
            Err(AssetsError::PathEscapesRoot(_))
        ));
        // …and the normalised form stays inside the root.
        assert!(!vfs.exists(&p("project/../secret.txt")));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_pointing_outside_are_rejected() {
        let tmp = TempDir::new("folder-symlink");
        tmp.file("secret.txt", b"secret");
        tmp.file("project/pkrs.toml", b"");
        std::os::unix::fs::symlink(tmp.0.join("secret.txt"), tmp.0.join("project/link.txt"))
            .expect("symlink");
        let vfs = FolderVfs::new(tmp.0.join("project")).expect("open");
        assert!(matches!(
            vfs.read(&p("link.txt")),
            Err(AssetsError::PathEscapesRoot(_))
        ));
    }

    #[test]
    fn root_must_be_an_existing_directory() {
        let tmp = TempDir::new("folder-root");
        tmp.file("file.txt", b"");
        assert!(FolderVfs::new(tmp.0.join("missing")).is_err());
        assert!(FolderVfs::new(tmp.0.join("file.txt")).is_err());
    }
}
