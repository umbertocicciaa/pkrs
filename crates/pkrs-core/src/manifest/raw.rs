//! Raw serde model of `pkrs.toml`: exact on-disk shape, defaults applied, nothing validated.
//!
//! Integers are read as `i64` (TOML's integer type) so range problems are reported by the
//! validator with a field path instead of as an opaque type error.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawManifest {
    pub game: RawGame,
    pub engine: RawEngine,
    #[serde(default)]
    pub display: RawDisplay,
    pub start: RawStart,
    #[serde(default)]
    pub features: RawFeatures,
    #[serde(default)]
    pub plugins: RawPlugins,
    #[serde(default)]
    pub locale: RawLocale,
    /// `action = ["key", …]` default bindings.
    #[serde(default)]
    pub input: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawGame {
    pub name: String,
    pub id: String,
    #[serde(default = "default_game_version")]
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
}

fn default_game_version() -> String {
    "0.1.0".to_owned()
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawEngine {
    pub version: String,
    pub api: i64,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct RawDisplay {
    pub width: i64,
    pub height: i64,
    pub scale: i64,
    pub fullscreen: bool,
}

impl Default for RawDisplay {
    fn default() -> Self {
        Self {
            width: 240,
            height: 160,
            scale: 3,
            fullscreen: false,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RawStart {
    pub map: String,
    pub position: [i64; 2],
    #[serde(default)]
    pub facing: Option<String>,
    #[serde(default)]
    pub script: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct RawFeatures {
    pub battles: bool,
    pub party: bool,
}

impl Default for RawFeatures {
    fn default() -> Self {
        Self {
            battles: true,
            party: true,
        }
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct RawPlugins {
    pub enabled: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(super) struct RawLocale {
    pub default: Option<String>,
    pub available: Option<Vec<String>>,
}
