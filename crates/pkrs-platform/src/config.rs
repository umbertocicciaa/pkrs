//! Per-user config file: bindings, volume and window scale (§10, §14.3).
//!
//! The file is `config.ron` in [`UserPaths::config_dir`](crate::UserPaths::config_dir). It is
//! not part of the save. Missing files are the caller's defaults; this module only parses,
//! merges and writes.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;

/// File name inside the config directory.
pub const CONFIG_FILE: &str = "config.ron";

/// Keys and buttons bound to one action. Either list may be empty; both empty is rejected
/// when the binding is applied.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserBinding {
    /// Keyboard names accepted by [`Key::parse_name`](crate::Key::parse_name).
    #[serde(default)]
    pub keys: Vec<String>,
    /// Button names accepted by [`GamepadButton::parse_name`](crate::GamepadButton::parse_name).
    #[serde(default)]
    pub buttons: Vec<String>,
}

/// Master / BGM / SFX / jingle volumes, linear `0.0..=1.0` (§11).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VolumeSettings {
    /// Scales every group.
    #[serde(default = "one")]
    pub master: f32,
    /// Background music.
    #[serde(default = "one")]
    pub bgm: f32,
    /// Sound effects.
    #[serde(default = "one")]
    pub sfx: f32,
    /// Jingles.
    #[serde(default = "one")]
    pub jingle: f32,
}

impl Default for VolumeSettings {
    fn default() -> Self {
        Self {
            master: 1.0,
            bgm: 1.0,
            sfx: 1.0,
            jingle: 1.0,
        }
    }
}

impl VolumeSettings {
    /// Clamps every group into `0.0..=1.0` (non-finite values become `0`).
    #[must_use]
    pub fn clamped(self) -> Self {
        Self {
            master: clamp_unit(self.master),
            bgm: clamp_unit(self.bgm),
            sfx: clamp_unit(self.sfx),
            jingle: clamp_unit(self.jingle),
        }
    }
}

fn one() -> f32 {
    1.0
}

fn clamp_unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Bindings, volumes and window scale stored per user (§14.3).
///
/// `window_scale` of `0` means "keep the project's `[display]` scale". A listed action in
/// `bindings` replaces that action's keys and buttons entirely; unlisted actions keep the
/// manifest and the built-in defaults.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct UserConfig {
    /// Action name → binding. Built-ins use `confirm`, plugins use `plugin:action`.
    #[serde(default)]
    pub bindings: BTreeMap<String, UserBinding>,
    /// Volume groups.
    #[serde(default)]
    pub volumes: VolumeSettings,
    /// Integer window scale. `0` does not override `pkrs.toml`.
    #[serde(default)]
    pub window_scale: u32,
}

impl UserConfig {
    /// Parses a RON document.
    pub fn parse(text: &str) -> Result<Self, ConfigError> {
        let mut config: Self =
            ron::from_str(text).map_err(|err| ConfigError::Parse(err.to_string()))?;
        config.volumes = config.volumes.clamped();
        Ok(config)
    }

    /// Pretty-printed RON.
    pub fn to_ron(&self) -> Result<String, ConfigError> {
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|err| ConfigError::Parse(err.to_string()))
    }

    /// Reads `config.ron` from `dir`. A missing file is [`ConfigError::NotFound`].
    pub fn read(dir: &Path) -> Result<Self, ConfigError> {
        let path = dir.join(CONFIG_FILE);
        let text = fs::read_to_string(&path).map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NotFound
            } else {
                ConfigError::Io(err.to_string())
            }
        })?;
        Self::parse(&text)
    }

    /// Writes `config.ron`, creating `dir` when needed.
    pub fn save(&self, dir: &Path) -> Result<(), ConfigError> {
        fs::create_dir_all(dir).map_err(|err| ConfigError::Io(err.to_string()))?;
        let path = dir.join(CONFIG_FILE);
        let text = self.to_ron()?;
        fs::write(&path, text).map_err(|err| ConfigError::Io(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_bindings_volume_and_scale() {
        let mut config = UserConfig {
            volumes: VolumeSettings {
                bgm: 0.4,
                ..VolumeSettings::default()
            },
            window_scale: 2,
            ..UserConfig::default()
        };
        config.bindings.insert(
            "confirm".to_owned(),
            UserBinding {
                keys: vec!["t".to_owned()],
                buttons: vec!["south".to_owned()],
            },
        );
        let parsed = UserConfig::parse(&config.to_ron().expect("ron")).expect("parse");
        assert_eq!(parsed, config);
    }

    #[test]
    fn missing_fields_use_defaults() {
        let config = UserConfig::parse("()").expect("empty struct");
        assert!(config.bindings.is_empty());
        assert_eq!(config.volumes, VolumeSettings::default());
        assert_eq!(config.window_scale, 0);
    }

    #[test]
    fn save_and_read_on_disk() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("pkrs-config-{nanos}"));
        let config = UserConfig {
            volumes: VolumeSettings {
                master: 0.5,
                ..VolumeSettings::default()
            },
            window_scale: 4,
            ..UserConfig::default()
        };
        config.save(&dir).expect("save");
        let loaded = UserConfig::read(&dir).expect("read");
        assert_eq!(loaded.window_scale, 4);
        assert_eq!(loaded.volumes.master, 0.5);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn volumes_outside_the_unit_interval_clamp() {
        let config =
            UserConfig::parse("(volumes: (master: 3.0, bgm: -1.0, sfx: 1.0, jingle: 0.2))")
                .expect("parse");
        assert_eq!(config.volumes.master, 1.0);
        assert_eq!(config.volumes.bgm, 0.0);
        assert_eq!(config.volumes.jingle, 0.2);
    }
}
