//! The `MapImporter` port (§4.5, §6.1) and its adapters.

#[cfg(feature = "tiled")]
mod tiled;

#[cfg(feature = "tiled")]
pub use self::tiled::TiledImporter;

use core::fmt;

use pkrs_assets::{AssetsError, Vfs, VfsPath};
use thiserror::Error;

use crate::conventions::ConventionViolation;
use crate::map::Map;

/// Why a map could not be imported.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MapError {
    /// The map or one of its tilesets could not be read from the VFS.
    #[error("cannot read `{path}`: {source}")]
    Read {
        /// File that failed.
        path: String,
        /// VFS error.
        #[source]
        source: AssetsError,
    },
    /// The file is not a valid map in the importer's format.
    #[error("cannot parse `{path}`: {message}")]
    Parse {
        /// File that failed.
        path: String,
        /// Parser message.
        message: String,
    },
    /// The map parsed but breaks the §6.2 conventions.
    #[error("map `{path}` breaks {} layer convention(s):\n{}", violations.len(), Violations(violations))]
    Conventions {
        /// Map file.
        path: String,
        /// Every violation found.
        violations: Vec<ConventionViolation>,
    },
}

impl MapError {
    /// The convention violations, if that is why the import failed (`pkrs check`).
    #[must_use]
    pub fn violations(&self) -> &[ConventionViolation] {
        match self {
            Self::Conventions { violations, .. } => violations,
            Self::Read { .. } | Self::Parse { .. } => &[],
        }
    }
}

struct Violations<'a>(&'a [ConventionViolation]);

impl fmt::Display for Violations<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, v) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "  - {v}")?;
        }
        Ok(())
    }
}

/// Engine-owned port: loads one map file through the [`Vfs`] into the engine [`Map`] model.
pub trait MapImporter {
    /// Imports the map at `path` (and every file it references) from `vfs`.
    fn import(&mut self, vfs: &dyn Vfs, path: &VfsPath) -> Result<Map, MapError>;
}

/// Map id for `path`: its file stem (`maps/route1.tmx` → `route1`).
#[must_use]
pub fn map_id_for(path: &VfsPath) -> String {
    let name = path.file_name().unwrap_or_default();
    match name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem.to_owned(),
        _ => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conventions::{ViolationKind, ViolationLocation};

    #[test]
    fn map_id_is_the_file_stem() {
        assert_eq!(
            map_id_for(&VfsPath::new("maps/route1.tmx").unwrap()),
            "route1"
        );
        assert_eq!(
            map_id_for(&VfsPath::new("a/b/house_1f.tmx").unwrap()),
            "house_1f"
        );
        assert_eq!(map_id_for(&VfsPath::new("noext").unwrap()), "noext");
    }

    #[test]
    fn convention_error_lists_every_violation() {
        let err = MapError::Conventions {
            path: "maps/a.tmx".into(),
            violations: vec![
                ConventionViolation {
                    location: ViolationLocation::Map,
                    kind: ViolationKind::MissingLayer("ground"),
                },
                ConventionViolation {
                    location: ViolationLocation::Layer("sky".into()),
                    kind: ViolationKind::UnsupportedLayer("image layer"),
                },
            ],
        };
        assert_eq!(err.violations().len(), 2);
        assert_eq!(
            err.to_string(),
            "map `maps/a.tmx` breaks 2 layer convention(s):\n  - map: required layer `ground` is missing\n  - layer `sky`: image layers are not supported"
        );
    }
}
