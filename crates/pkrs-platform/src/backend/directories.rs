//! `DirectoriesPaths`: the `UserPaths` adapter over the `directories` crate (§14.3).
//!
//! This is the **only** module of `pkrs-platform` that imports `directories`.

use std::path::{Path, PathBuf};

use ::directories::ProjectDirs;

use crate::paths::{UserPaths, split_game_id};
use crate::{PlatformError, Result};

/// Per-OS user directories keyed on `game.id` (e.g. `com.example.lighthouse`).
///
/// - data (saves): `ProjectDirs::data_dir()`
/// - config: `ProjectDirs::config_dir()`
/// - logs: `ProjectDirs::data_local_dir()/logs`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirectoriesPaths {
    data: PathBuf,
    config: PathBuf,
    logs: PathBuf,
}

impl DirectoriesPaths {
    /// Resolves the directories for `game_id`.
    ///
    /// Fails with [`PlatformError::InvalidGameId`] for a malformed id and
    /// [`PlatformError::UserPathsUnavailable`] when the OS has no home directory.
    pub fn new(game_id: &str) -> Result<Self> {
        let (qualifier, organization, application) = split_game_id(game_id)
            .ok_or_else(|| PlatformError::InvalidGameId(game_id.to_owned()))?;
        let dirs = ProjectDirs::from(&qualifier, &organization, &application)
            .ok_or(PlatformError::UserPathsUnavailable)?;
        Ok(Self {
            data: dirs.data_dir().to_path_buf(),
            config: dirs.config_dir().to_path_buf(),
            logs: dirs.data_local_dir().join("logs"),
        })
    }
}

impl UserPaths for DirectoriesPaths {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(parts: &[&str]) -> PathBuf {
        parts.iter().collect()
    }

    #[test]
    fn rejects_malformed_ids() {
        assert!(matches!(
            DirectoriesPaths::new(""),
            Err(PlatformError::InvalidGameId(_))
        ));
        assert!(matches!(
            DirectoriesPaths::new("com..x"),
            Err(PlatformError::InvalidGameId(_))
        ));
    }

    #[test]
    fn logs_live_in_a_logs_subdirectory() {
        let paths = DirectoriesPaths::new("com.example.lighthouse").unwrap();
        assert_eq!(paths.logs_dir().file_name(), Some("logs".as_ref()));
    }

    #[test]
    fn different_ids_get_different_dirs() {
        let a = DirectoriesPaths::new("com.example.lighthouse").unwrap();
        let b = DirectoriesPaths::new("com.example.harbour").unwrap();
        assert_ne!(a.data_dir(), b.data_dir());
        assert_ne!(a.config_dir(), b.config_dir());
        assert_ne!(a.logs_dir(), b.logs_dir());
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_paths_are_keyed_on_org_and_app() {
        // %APPDATA%\<org>\<app>\{data,config}, %LOCALAPPDATA%\<org>\<app>\data\logs
        let paths = DirectoriesPaths::new("com.example.lighthouse").unwrap();
        assert!(
            paths
                .data_dir()
                .ends_with(rel(&["example", "lighthouse", "data"]))
        );
        assert!(
            paths
                .config_dir()
                .ends_with(rel(&["example", "lighthouse", "config"]))
        );
        assert!(
            paths
                .logs_dir()
                .ends_with(rel(&["example", "lighthouse", "data", "logs"]))
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_paths_are_keyed_on_bundle_id() {
        // ~/Library/Application Support/<id> for data, config and data_local (logs).
        let paths = DirectoriesPaths::new("com.example.lighthouse").unwrap();
        assert!(
            paths
                .data_dir()
                .ends_with(rel(&["Application Support", "com.example.lighthouse"]))
        );
        assert!(paths.config_dir().ends_with("com.example.lighthouse"));
        assert!(
            paths
                .logs_dir()
                .ends_with(rel(&["com.example.lighthouse", "logs"]))
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    #[test]
    fn xdg_paths_are_keyed_on_app_name() {
        // $XDG_DATA_HOME/<app>, $XDG_CONFIG_HOME/<app>, $XDG_DATA_HOME/<app>/logs
        let paths = DirectoriesPaths::new("com.example.lighthouse").unwrap();
        assert!(paths.data_dir().ends_with("lighthouse"));
        assert!(paths.config_dir().ends_with("lighthouse"));
        assert!(paths.logs_dir().ends_with(rel(&["lighthouse", "logs"])));
    }
}
