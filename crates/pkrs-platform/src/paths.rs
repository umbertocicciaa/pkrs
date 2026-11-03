//! The `UserPaths` port: per-user directories for saves, config and logs (§14.3).

use std::path::{Path, PathBuf};

/// Engine-owned port for per-user, per-game directories.
///
/// Directories are not created by the port; callers create them before writing.
pub trait UserPaths {
    /// Where saves live.
    fn data_dir(&self) -> &Path;
    /// Where per-user config lives (bindings, volume, window scale).
    fn config_dir(&self) -> &Path;
    /// Where log files are written.
    fn logs_dir(&self) -> &Path;
}

/// [`UserPaths`] rooted at a fixed directory: `<root>/data`, `<root>/config`, `<root>/logs`.
///
/// Used by tests, `--headless` runs and portable installs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixedPaths {
    data: PathBuf,
    config: PathBuf,
    logs: PathBuf,
}

impl FixedPaths {
    /// Lays the three directories out under `root`.
    pub fn under(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        Self {
            data: root.join("data"),
            config: root.join("config"),
            logs: root.join("logs"),
        }
    }
}

impl UserPaths for FixedPaths {
    fn data_dir(&self) -> &Path {
        &self.data
    }

    fn config_dir(&self) -> &Path {
        &self.config
    }

    fn logs_dir(&self) -> &Path {
        &self.logs
    }
}

/// Splits a reverse-DNS `game.id` into `(qualifier, organization, application)`.
///
/// `com.example.lighthouse` → `("com", "example", "lighthouse")`; extra middle segments
/// are kept in the organization (`org.acme.games.tale` → `("org", "acme.games", "tale")`);
/// shorter ids leave the leading parts empty. Returns `None` for an empty id or an empty
/// segment.
#[cfg(any(feature = "directories", test))]
pub(crate) fn split_game_id(id: &str) -> Option<(String, String, String)> {
    let segments: Vec<&str> = id.split('.').collect();
    if segments.iter().any(|s| s.trim().is_empty()) {
        return None;
    }
    match segments.as_slice() {
        [app] => Some((String::new(), String::new(), (*app).to_owned())),
        [org, app] => Some((String::new(), (*org).to_owned(), (*app).to_owned())),
        [qualifier, middle @ .., app] => {
            Some(((*qualifier).to_owned(), middle.join("."), (*app).to_owned()))
        }
        [] => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_paths_are_under_root() {
        let paths = FixedPaths::under("/tmp/game");
        assert_eq!(paths.data_dir(), Path::new("/tmp/game").join("data"));
        assert_eq!(paths.config_dir(), Path::new("/tmp/game").join("config"));
        assert_eq!(paths.logs_dir(), Path::new("/tmp/game").join("logs"));
    }

    #[test]
    fn split_game_id_handles_reverse_dns() {
        let s = |a: &str, b: &str, c: &str| Some((a.to_owned(), b.to_owned(), c.to_owned()));
        assert_eq!(
            split_game_id("com.example.lighthouse"),
            s("com", "example", "lighthouse")
        );
        assert_eq!(
            split_game_id("org.acme.games.tale"),
            s("org", "acme.games", "tale")
        );
        assert_eq!(split_game_id("acme.tale"), s("", "acme", "tale"));
        assert_eq!(split_game_id("tale"), s("", "", "tale"));
        assert_eq!(split_game_id(""), None);
        assert_eq!(split_game_id("com..tale"), None);
    }
}
