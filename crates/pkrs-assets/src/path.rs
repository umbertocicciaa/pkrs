//! Virtual asset paths and their normalisation rules (§13).
//!
//! **Rules** (the same on every OS, so a game that works on one works on all):
//! - Segments are separated by `/`; `\` is accepted on input and converted to `/`.
//! - Empty segments and `.` are dropped; `..` removes the previous segment. A `..` that would
//!   climb above the mount root is rejected ([`AssetsError::PathEscapesRoot`]).
//! - Absolute paths (`/x`, `\x`) and Windows drive prefixes are rejected.
//! - Segments must not contain control characters or any of `: * ? " < > |`.
//! - Paths are **case-sensitive**. `FolderVfs` enforces the exact on-disk case even on
//!   case-insensitive filesystems (Windows, default macOS), so case bugs show up in development
//!   instead of on a player's Linux machine.
//! - An [`AssetPath`] may start with a `plugin_id:` mount prefix (`resonance:ui/icon.png`).

use core::fmt;

use crate::error::{AssetsError, Result};

/// A normalised path relative to one mount root (no prefix). The empty path is the root.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VfsPath(String);

impl VfsPath {
    /// The mount root (empty path); only meaningful for directory listing.
    pub fn root() -> Self {
        Self(String::new())
    }

    /// Normalises `raw` (see module docs).
    pub fn new(raw: &str) -> Result<Self> {
        let invalid = |reason: &str| AssetsError::InvalidPath {
            path: raw.to_owned(),
            reason: reason.to_owned(),
        };
        if raw.starts_with('/') || raw.starts_with('\\') {
            return Err(invalid("absolute paths are not allowed"));
        }
        let mut segments: Vec<&str> = Vec::new();
        for segment in raw.split(['/', '\\']) {
            match segment {
                "" | "." => {}
                ".." => {
                    if segments.pop().is_none() {
                        return Err(AssetsError::PathEscapesRoot(raw.to_owned()));
                    }
                }
                _ => {
                    if let Some(c) = segment.chars().find(|&c| is_forbidden(c)) {
                        return Err(invalid(&format!("forbidden character {c:?}")));
                    }
                    segments.push(segment);
                }
            }
        }
        Ok(Self(segments.join("/")))
    }

    /// The normalised path string (`sprites/hero.png`; empty for the root).
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` for the mount root.
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the path segments.
    pub fn segments(&self) -> impl Iterator<Item = &str> {
        self.0.split('/').filter(|s| !s.is_empty())
    }

    /// The final segment (file or directory name); `None` for the root.
    pub fn file_name(&self) -> Option<&str> {
        self.segments().last()
    }

    /// Appends one already-valid child name (used when listing directories).
    pub(crate) fn join_name(&self, name: &str) -> Result<Self> {
        if self.is_root() {
            Self::new(name)
        } else {
            Self::new(&format!("{}/{name}", self.0))
        }
    }

    /// Returns `true` if `self` is directly or transitively inside directory `dir`.
    pub fn starts_with_dir(&self, dir: &VfsPath) -> bool {
        dir.is_root()
            || self
                .0
                .strip_prefix(dir.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
    }
}

impl fmt::Display for VfsPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn is_forbidden(c: char) -> bool {
    c.is_control() || matches!(c, ':' | '*' | '?' | '"' | '<' | '>' | '|')
}

/// A full asset path: an optional `plugin_id:` mount prefix plus a [`VfsPath`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetPath {
    mount: Option<String>,
    path: VfsPath,
}

impl AssetPath {
    /// Parses `"sprites/hero.png"` or `"resonance:ui/icon.png"`.
    pub fn parse(raw: &str) -> Result<Self> {
        match raw.split_once(':') {
            None => Ok(Self {
                mount: None,
                path: VfsPath::new(raw)?,
            }),
            Some((mount, rest)) => {
                if !is_mount_id(mount) {
                    return Err(AssetsError::InvalidPath {
                        path: raw.to_owned(),
                        reason: format!(
                            "`{mount}` is not a plugin id (lowercase letter first, then lowercase letters, digits, `_` or `-`)"
                        ),
                    });
                }
                Ok(Self {
                    mount: Some(mount.to_owned()),
                    path: VfsPath::new(rest)?,
                })
            }
        }
    }

    /// Builds a path inside the project mount.
    pub fn project(path: VfsPath) -> Self {
        Self { mount: None, path }
    }

    /// Builds a path inside a plugin mount.
    pub fn in_mount(mount: &str, path: VfsPath) -> Result<Self> {
        if !is_mount_id(mount) {
            return Err(AssetsError::InvalidPath {
                path: format!("{mount}:{path}"),
                reason: format!("`{mount}` is not a plugin id"),
            });
        }
        Ok(Self {
            mount: Some(mount.to_owned()),
            path,
        })
    }

    /// The mount prefix (`None` for project files).
    pub fn mount(&self) -> Option<&str> {
        self.mount.as_deref()
    }

    /// The path inside the mount.
    pub fn path(&self) -> &VfsPath {
        &self.path
    }
}

impl fmt::Display for AssetPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.mount {
            Some(mount) => write!(f, "{mount}:{}", self.path),
            None => self.path.fmt(f),
        }
    }
}

/// Mount prefixes are plugin ids: lowercase letter first, then `[a-z0-9_-]`.
pub(crate) fn is_mount_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn norm(raw: &str) -> String {
        VfsPath::new(raw).expect("valid").as_str().to_owned()
    }

    #[test]
    fn normalises_separators_dots_and_empty_segments() {
        assert_eq!(norm("sprites/hero.png"), "sprites/hero.png");
        assert_eq!(norm("sprites\\hero.png"), "sprites/hero.png");
        assert_eq!(norm("./sprites//npc/./hero.png"), "sprites/npc/hero.png");
        assert_eq!(norm("sprites/npc/../hero.png"), "sprites/hero.png");
        assert_eq!(norm("sprites/"), "sprites");
        assert_eq!(norm(""), "");
        assert_eq!(norm("a/.."), "");
        assert!(VfsPath::new(".").expect("valid").is_root());
    }

    #[test]
    fn traversal_above_root_is_rejected() {
        for raw in ["..", "../secret", "a/../../b", "sprites\\..\\..\\x"] {
            assert!(
                matches!(VfsPath::new(raw), Err(AssetsError::PathEscapesRoot(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn absolute_and_forbidden_paths_are_rejected() {
        for raw in [
            "/etc/passwd",
            "\\x",
            "a/b?.png",
            "a/<b>",
            "a\u{0}b",
            "a/b|c",
        ] {
            assert!(
                matches!(VfsPath::new(raw), Err(AssetsError::InvalidPath { .. })),
                "{raw}"
            );
        }
    }

    #[test]
    fn case_is_preserved() {
        assert_eq!(norm("Sprites/Hero.PNG"), "Sprites/Hero.PNG");
        assert_ne!(VfsPath::new("a.png").ok(), VfsPath::new("A.png").ok());
    }

    #[test]
    fn segments_and_file_name() {
        let p = VfsPath::new("sprites/npc/hero.png").expect("valid");
        assert_eq!(
            p.segments().collect::<Vec<_>>(),
            ["sprites", "npc", "hero.png"]
        );
        assert_eq!(p.file_name(), Some("hero.png"));
        assert_eq!(VfsPath::root().file_name(), None);
    }

    #[test]
    fn starts_with_dir_respects_segment_boundaries() {
        let p = VfsPath::new("sprites/hero.png").expect("valid");
        assert!(p.starts_with_dir(&VfsPath::new("sprites").expect("valid")));
        assert!(p.starts_with_dir(&VfsPath::root()));
        assert!(!p.starts_with_dir(&VfsPath::new("sprite").expect("valid")));
        assert!(!p.starts_with_dir(&p));
    }

    #[test]
    fn asset_path_parses_mount_prefix() {
        let p = AssetPath::parse("resonance:ui/icon.png").expect("valid");
        assert_eq!(p.mount(), Some("resonance"));
        assert_eq!(p.path().as_str(), "ui/icon.png");
        assert_eq!(p.to_string(), "resonance:ui/icon.png");

        let p = AssetPath::parse("day-night:./a/../b.ron").expect("valid");
        assert_eq!(p.to_string(), "day-night:b.ron");

        let p = AssetPath::parse("sprites/hero.png").expect("valid");
        assert_eq!(p.mount(), None);
        assert_eq!(
            p,
            AssetPath::project(VfsPath::new("sprites/hero.png").expect("valid"))
        );
    }

    #[test]
    fn asset_path_rejects_bad_prefixes_and_escapes() {
        assert!(AssetPath::parse("C:/Windows/x").is_err());
        assert!(AssetPath::parse(":x").is_err());
        assert!(AssetPath::parse("a/b:c").is_err());
        assert!(AssetPath::parse("res:a:b").is_err());
        assert!(matches!(
            AssetPath::parse("resonance:../../pkrs.toml"),
            Err(AssetsError::PathEscapesRoot(_))
        ));
        assert!(AssetPath::in_mount("Bad", VfsPath::root()).is_err());
    }
}
