//! The project manifest, `pkrs.toml` (§2.2), shared by the runtime and the CLI.
//!
//! Loading has two stages:
//! 1. **Parse** (`toml_codec`): TOML syntax, value types and unknown keys. Failures become
//!    [`ManifestError::Parse`] with the field path and line/column.
//! 2. **Validate**: ranges, id formats, semver, cross-field rules. *All* problems are collected
//!    into [`ManifestError::Invalid`], each with its dotted field path.
//!
//! Plugin manifests (`plugins/<id>/plugin.toml`, §9.4) use the same two stages; see
//! [`PluginManifest`].
//!
//! Only `pkrs.toml` is required in a project; every folder is optional (§2.1). A minimal manifest
//! needs just `[game]` (`name`, `id`), `[engine]` (`version`, `api`) and `[start]` (`map`,
//! `position`); everything else has defaults.

pub mod diagnostic;
mod error;
mod plugin;
mod raw;
mod toml_codec;

use std::collections::{BTreeMap, BTreeSet};
use std::ops::RangeInclusive;
use std::path::Path;

pub use error::{Location, ManifestError, ManifestIssue, PluginManifestError};
pub use plugin::{PluginInfo, PluginLoadRules, PluginManifest};

use crate::direction::Direction;
use crate::grid::GridPos;
use crate::id::validate_id;
use crate::version::{Version, VersionReq};
use raw::RawManifest;

/// Valid range for `display.width` and `display.height` (pixels).
pub const DISPLAY_SIZE_RANGE: RangeInclusive<u32> = 1..=4096;
/// Valid range for `display.scale`.
pub const DISPLAY_SCALE_RANGE: RangeInclusive<u32> = 1..=16;

/// The parsed and validated `pkrs.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProjectManifest {
    /// `[game]`: identity of the game.
    pub game: GameInfo,
    /// `[engine]`: required engine version and scripting API level.
    pub engine: EngineRequirement,
    /// `[display]`: internal resolution and initial window mode.
    pub display: DisplayConfig,
    /// `[start]`: where a new game begins.
    pub start: StartConfig,
    /// `[features]`: engine subsystems the game uses.
    pub features: Features,
    /// `[plugins]`: enabled plugins.
    pub plugins: PluginsConfig,
    /// `[locale]`: languages.
    pub locale: LocaleConfig,
    /// `[input]`: default key bindings per action.
    pub input: InputConfig,
}

/// `[game]` table.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct GameInfo {
    /// Display name (window title).
    pub name: String,
    /// Reverse-DNS id (`com.example.lighthouse`), used for save/config paths and the macOS bundle id.
    pub id: String,
    /// Game version (default `0.1.0`).
    pub version: Version,
    /// Authors (default empty).
    pub authors: Vec<String>,
}

/// `[engine]` table (§2.5).
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct EngineRequirement {
    /// Semver requirement on the engine/runtime (`"0.3"` means `^0.3`).
    pub version: VersionReq,
    /// Scripting API level the game's scripts target (§9.7); independent of engine semver.
    pub api: u32,
}

impl EngineRequirement {
    /// Returns `true` if an engine of `version` satisfies `engine.version`.
    pub fn accepts(&self, version: &Version) -> bool {
        self.version.matches(version)
    }
}

/// `[display]` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct DisplayConfig {
    /// Internal render width in pixels (default 240).
    pub width: u32,
    /// Internal render height in pixels (default 160).
    pub height: u32,
    /// Initial integer window scale (default 3).
    pub scale: u32,
    /// Start in fullscreen (default `false`).
    pub fullscreen: bool,
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            width: 240,
            height: 160,
            scale: 3,
            fullscreen: false,
        }
    }
}

/// `[start]` table.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct StartConfig {
    /// Map id the new game starts on.
    pub map: String,
    /// Starting tile.
    pub position: GridPos,
    /// Starting facing (default `down`).
    pub facing: Direction,
    /// Optional project-relative game-level init script (forward slashes, `.lua`).
    pub script: Option<String>,
}

/// `[features]` table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Features {
    /// Battle subsystem (default `true`); `false` skips loading battle data.
    pub battles: bool,
    /// Party subsystem (default `true`).
    pub party: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            battles: true,
            party: true,
        }
    }
}

/// `[plugins]` table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginsConfig {
    /// Enabled plugin ids, as a load-order hint (final order is resolved, §9.4).
    pub enabled: Vec<String>,
}

/// `[locale]` table.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct LocaleConfig {
    /// Default language tag (default `en`).
    pub default: String,
    /// All shipped languages (default: just `default`); always contains `default`.
    pub available: Vec<String>,
}

impl Default for LocaleConfig {
    fn default() -> Self {
        Self {
            default: "en".to_owned(),
            available: vec!["en".to_owned()],
        }
    }
}

/// `[input]` table: default bindings, `action = ["key", …]` (§10).
///
/// Action names are `snake_case` built-ins (`confirm`) or plugin-namespaced (`resonance:toggle`).
/// Key names are resolved against the platform key table by the `Input` facade, not here.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct InputConfig {
    /// Action → key names, sorted by action.
    pub bindings: BTreeMap<String, Vec<String>>,
}

impl ProjectManifest {
    /// File name of the manifest at the project root.
    pub const FILE_NAME: &'static str = "pkrs.toml";

    /// Parses and validates manifest source text.
    pub fn from_toml_str(src: &str) -> Result<Self, ManifestError> {
        let raw = toml_codec::parse(src).map_err(ManifestError::Parse)?;
        validate(raw)
    }

    /// Reads and validates `<project_dir>/pkrs.toml`.
    pub fn load(project_dir: &Path) -> Result<Self, ManifestError> {
        let path = project_dir.join(Self::FILE_NAME);
        let src =
            std::fs::read_to_string(&path).map_err(|source| ManifestError::Io { path, source })?;
        Self::from_toml_str(&src)
    }

    /// Checks the manifest against a concrete engine: `engine.version` must accept `engine`
    /// and `engine.api` must be in `supported_api` (§2.5, §9.7).
    pub fn check_engine(
        &self,
        engine: &Version,
        supported_api: RangeInclusive<u32>,
    ) -> Result<(), ManifestError> {
        let mut issues = Vec::new();
        if !self.engine.accepts(engine) {
            issues.push(ManifestIssue::new(
                "engine.version",
                format!(
                    "requirement `{}` is not satisfied by engine {engine}",
                    self.engine.version
                ),
            ));
        }
        if !supported_api.contains(&self.engine.api) {
            issues.push(ManifestIssue::new(
                "engine.api",
                format!(
                    "API level {} is not supported by engine {engine} (supported: {}..={})",
                    self.engine.api,
                    supported_api.start(),
                    supported_api.end()
                ),
            ));
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(ManifestError::Invalid(issues))
        }
    }
}

/// Collects validation issues while building the typed manifest.
#[derive(Default)]
struct Validator {
    issues: Vec<ManifestIssue>,
}

impl Validator {
    fn push(&mut self, path: impl Into<String>, message: impl Into<String>) {
        self.issues.push(ManifestIssue::new(path, message));
    }

    fn non_empty(&mut self, path: &str, value: &str) {
        if value.trim().is_empty() {
            self.push(path, "must not be empty");
        }
    }

    fn ranged_u32(&mut self, path: &str, value: i64, range: RangeInclusive<u32>) -> u32 {
        match u32::try_from(value) {
            Ok(v) if range.contains(&v) => v,
            _ => {
                self.push(
                    path,
                    format!(
                        "must be between {} and {} (got {value})",
                        range.start(),
                        range.end()
                    ),
                );
                *range.start()
            }
        }
    }

    fn coordinate(&mut self, path: &str, value: i64) -> i32 {
        match i32::try_from(value) {
            Ok(v) if v >= 0 => v,
            _ => {
                self.push(
                    path,
                    format!("must be a non-negative tile coordinate (got {value})"),
                );
                0
            }
        }
    }

    fn unique<'a>(&mut self, path: &str, items: impl IntoIterator<Item = &'a String>) {
        let mut seen = BTreeSet::new();
        for (i, item) in items.into_iter().enumerate() {
            if !seen.insert(item.as_str()) {
                self.push(format!("{path}[{i}]"), format!("duplicate entry `{item}`"));
            }
        }
    }
}

fn validate(raw: RawManifest) -> Result<ProjectManifest, ManifestError> {
    let mut v = Validator::default();

    // [game]
    v.non_empty("game.name", &raw.game.name);
    if let Err(message) = check_game_id(&raw.game.id) {
        v.push("game.id", message);
    }
    let game_version = Version::parse(&raw.game.version).unwrap_or_else(|e| {
        v.push("game.version", e.to_string());
        Version::new(0, 0, 0)
    });
    for (i, author) in raw.game.authors.iter().enumerate() {
        v.non_empty(&format!("game.authors[{i}]"), author);
    }

    // [engine]
    let engine_version = VersionReq::parse(&raw.engine.version).ok();
    if engine_version.is_none() {
        v.push(
            "engine.version",
            format!(
                "`{}` is not a semver requirement (e.g. \"0.3\" or \"^1.2\")",
                raw.engine.version
            ),
        );
    }
    let api = v.ranged_u32("engine.api", raw.engine.api, 1..=u32::MAX);

    // [display]
    let display = DisplayConfig {
        width: v.ranged_u32("display.width", raw.display.width, DISPLAY_SIZE_RANGE),
        height: v.ranged_u32("display.height", raw.display.height, DISPLAY_SIZE_RANGE),
        scale: v.ranged_u32("display.scale", raw.display.scale, DISPLAY_SCALE_RANGE),
        fullscreen: raw.display.fullscreen,
    };

    // [start]
    if validate_id(&raw.start.map).is_err() {
        v.push(
            "start.map",
            format!(
                "`{}` is not a valid map id (lowercase letter first, then lowercase letters, digits or `_`)",
                raw.start.map
            ),
        );
    }
    let position = GridPos::new(
        v.coordinate("start.position[0]", raw.start.position[0]),
        v.coordinate("start.position[1]", raw.start.position[1]),
    );
    let facing = match raw.start.facing.as_deref() {
        None => Direction::Down,
        Some(raw_facing) => raw_facing.parse().unwrap_or_else(|_| {
            v.push(
                "start.facing",
                format!("`{raw_facing}` is not one of up, down, left, right"),
            );
            Direction::Down
        }),
    };
    if let Some(script) = &raw.start.script
        && let Err(message) = check_script_path(script)
    {
        v.push("start.script", message);
    }

    // [plugins]
    for (i, id) in raw.plugins.enabled.iter().enumerate() {
        if !is_plugin_id(id) {
            v.push(
                format!("plugins.enabled[{i}]"),
                format!(
                    "`{id}` is not a valid plugin id (lowercase letter first, then lowercase letters, digits, `_` or `-`)"
                ),
            );
        }
    }
    v.unique("plugins.enabled", &raw.plugins.enabled);

    // [locale]
    let default_locale = raw.locale.default.unwrap_or_else(|| "en".to_owned());
    if !is_language_tag(&default_locale) {
        v.push(
            "locale.default",
            format!("`{default_locale}` is not a language tag (e.g. \"en\" or \"pt-BR\")"),
        );
    }
    let available = raw
        .locale
        .available
        .unwrap_or_else(|| vec![default_locale.clone()]);
    for (i, tag) in available.iter().enumerate() {
        if !is_language_tag(tag) {
            v.push(
                format!("locale.available[{i}]"),
                format!("`{tag}` is not a language tag (e.g. \"en\" or \"pt-BR\")"),
            );
        }
    }
    v.unique("locale.available", &available);
    if !available.contains(&default_locale) {
        v.push(
            "locale.available",
            format!("must contain the default locale `{default_locale}`"),
        );
    }

    // [input]
    for (action, keys) in &raw.input {
        let path = format!("input.{action}");
        if !is_action_name(action) {
            v.push(
                path.as_str(),
                format!("`{action}` is not a valid action name (`snake_case` or `plugin:action`)"),
            );
        }
        if keys.is_empty() {
            v.push(path.as_str(), "must list at least one key");
        }
        for (i, key) in keys.iter().enumerate() {
            v.non_empty(&format!("{path}[{i}]"), key);
        }
        v.unique(&path, keys);
    }

    match engine_version {
        Some(version) if v.issues.is_empty() => Ok(ProjectManifest {
            game: GameInfo {
                name: raw.game.name,
                id: raw.game.id,
                version: game_version,
                authors: raw.game.authors,
            },
            engine: EngineRequirement { version, api },
            display,
            start: StartConfig {
                map: raw.start.map,
                position,
                facing,
                script: raw.start.script,
            },
            features: Features {
                battles: raw.features.battles,
                party: raw.features.party,
            },
            plugins: PluginsConfig {
                enabled: raw.plugins.enabled,
            },
            locale: LocaleConfig {
                default: default_locale,
                available,
            },
            input: InputConfig {
                bindings: raw.input,
            },
        }),
        _ => Err(ManifestError::Invalid(v.issues)),
    }
}

/// `game.id`: dot-separated segments of ASCII letters, digits, `_` or `-`.
fn check_game_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("must not be empty".to_owned());
    }
    let segments_ok = id.split('.').all(|seg| {
        !seg.is_empty()
            && seg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    });
    if segments_ok {
        Ok(())
    } else {
        Err(format!(
            "`{id}` is not a reverse-DNS id (e.g. \"com.example.lighthouse\"): use dot-separated segments of ASCII letters, digits, `_` or `-`"
        ))
    }
}

/// `start.script`: relative, forward slashes, no `..`, ends in `.lua`.
fn check_script_path(path: &str) -> Result<(), String> {
    if path.contains('\\') {
        return Err(format!("`{path}` must use forward slashes"));
    }
    if path.starts_with('/') || path.contains(':') {
        return Err(format!("`{path}` must be relative to the project root"));
    }
    if path
        .split('/')
        .any(|seg| seg.is_empty() || seg == "." || seg == "..")
    {
        return Err(format!(
            "`{path}` must not contain empty, `.` or `..` segments"
        ));
    }
    if !path.ends_with(".lua") {
        return Err(format!("`{path}` must be a `.lua` file"));
    }
    Ok(())
}

/// Plugin ids: like content ids, but `-` is allowed (`day-night`).
pub fn is_plugin_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-')
}

/// BCP-47-ish language tag: `-`-separated alphanumeric subtags, the first 2–8 letters.
fn is_language_tag(tag: &str) -> bool {
    let mut subtags = tag.split('-');
    let primary_ok = subtags
        .next()
        .is_some_and(|p| (2..=8).contains(&p.len()) && p.chars().all(|c| c.is_ascii_alphabetic()));
    primary_ok
        && subtags
            .all(|s| (1..=8).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// Action names: `snake_case` or `plugin_id:snake_case`.
pub fn is_action_name(name: &str) -> bool {
    match name.split_once(':') {
        Some((plugin, action)) => is_plugin_id(plugin) && validate_id(action).is_ok(),
        None => validate_id(name).is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = r#"
[game]
name = "Lighthouse Tale"
id   = "com.example.lighthouse"

[engine]
version = "0.3"
api     = 1

[start]
map      = "lighthouse_1f"
position = [5, 7]
"#;

    const FULL: &str = r#"
[game]
name    = "Lighthouse Tale"
id      = "com.example.lighthouse"
version = "1.2.3"
authors = ["Jane Doe", "Ada"]

[engine]
version = "^0.3.1"
api     = 2

[display]
width      = 320
height     = 180
scale      = 4
fullscreen = true

[start]
map      = "lighthouse_1f"
position = [5, 7]
facing   = "left"
script   = "scripts/main.lua"

[features]
battles = false

[plugins]
enabled = ["resonance", "day-night"]

[locale]
default   = "it"
available = ["en", "it", "pt-BR"]

[input]
confirm            = ["z", "enter"]
"resonance:toggle" = ["r"]
"#;

    fn issues_of(src: &str) -> Vec<ManifestIssue> {
        match ProjectManifest::from_toml_str(src) {
            Ok(m) => panic!("expected an error, got {m:?}"),
            Err(e) => e.issues().to_vec(),
        }
    }

    fn paths_of(src: &str) -> Vec<String> {
        issues_of(src).into_iter().map(|i| i.path).collect()
    }

    fn with(replace: &str, by: &str) -> String {
        assert!(FULL.contains(replace), "fixture lacks {replace:?}");
        FULL.replacen(replace, by, 1)
    }

    #[test]
    fn minimal_manifest_loads_with_defaults() {
        let m = ProjectManifest::from_toml_str(MINIMAL).expect("valid");
        assert_eq!(m.game.name, "Lighthouse Tale");
        assert_eq!(m.game.version, Version::new(0, 1, 0));
        assert!(m.game.authors.is_empty());
        assert_eq!(m.engine.api, 1);
        assert_eq!(m.display, DisplayConfig::default());
        assert_eq!(
            (m.display.width, m.display.height, m.display.scale),
            (240, 160, 3)
        );
        assert_eq!(m.start.position, GridPos::new(5, 7));
        assert_eq!(m.start.facing, Direction::Down);
        assert_eq!(m.start.script, None);
        assert_eq!(m.features, Features::default());
        assert!(m.features.battles && m.features.party);
        assert!(m.plugins.enabled.is_empty());
        assert_eq!(m.locale, LocaleConfig::default());
        assert!(m.input.bindings.is_empty());
    }

    #[test]
    fn full_manifest_round_trips_every_section() {
        let m = ProjectManifest::from_toml_str(FULL).expect("valid");
        assert_eq!(m.game.version, Version::new(1, 2, 3));
        assert_eq!(m.game.authors, ["Jane Doe", "Ada"]);
        assert_eq!(m.engine.api, 2);
        assert_eq!(
            m.display,
            DisplayConfig {
                width: 320,
                height: 180,
                scale: 4,
                fullscreen: true
            }
        );
        assert_eq!(m.start.facing, Direction::Left);
        assert_eq!(m.start.script.as_deref(), Some("scripts/main.lua"));
        assert!(!m.features.battles);
        assert!(m.features.party, "unset feature keeps its default");
        assert_eq!(m.plugins.enabled, ["resonance", "day-night"]);
        assert_eq!(m.locale.default, "it");
        assert_eq!(m.locale.available, ["en", "it", "pt-BR"]);
        assert_eq!(m.input.bindings["confirm"], ["z", "enter"]);
        assert_eq!(m.input.bindings["resonance:toggle"], ["r"]);
    }

    #[test]
    fn locale_available_defaults_to_default() {
        let src = format!("{MINIMAL}\n[locale]\ndefault = \"it\"\n");
        let m = ProjectManifest::from_toml_str(&src).expect("valid");
        assert_eq!(m.locale.available, ["it"]);
    }

    #[test]
    fn engine_requirement_matching() {
        let m = ProjectManifest::from_toml_str(MINIMAL).expect("valid");
        assert!(m.engine.accepts(&Version::new(0, 3, 0)));
        assert!(m.engine.accepts(&Version::new(0, 3, 7)));
        assert!(!m.engine.accepts(&Version::new(0, 4, 0)));
        assert!(m.check_engine(&Version::new(0, 3, 2), 1..=1).is_ok());

        let err = m
            .check_engine(&Version::new(0, 4, 0), 2..=3)
            .expect_err("incompatible");
        let paths: Vec<_> = err.issues().iter().map(|i| i.path.as_str()).collect();
        assert_eq!(paths, ["engine.version", "engine.api"]);
        assert!(err.to_string().contains("not satisfied by engine 0.4.0"));
        assert!(err.to_string().contains("supported: 2..=3"));
    }

    #[test]
    fn missing_required_tables_and_keys_are_parse_errors() {
        let err = ProjectManifest::from_toml_str("[game]\nname = \"x\"\n").expect_err("invalid");
        assert!(matches!(err, ManifestError::Parse(_)), "{err:?}");
        let issue = &err.issues()[0];
        assert_eq!(issue.path, "game");
        assert!(issue.message.contains("missing field `id`"), "{issue}");

        let no_start = MINIMAL.replace("[start]", "[nope]");
        let issue = &issues_of(&no_start)[0];
        assert_eq!(issue.path, "nope");
        assert!(issue.message.contains("unknown field `nope`"), "{issue}");
    }

    #[test]
    fn type_errors_carry_field_path_and_location() {
        let src = with("scale      = 4", "scale      = \"4\"");
        let issues = issues_of(&src);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].path, "display.scale");
        let loc = issues[0].location.expect("has a location");
        assert_eq!(loc.line, 15, "{}", issues[0]);
    }

    #[test]
    fn unknown_keys_are_rejected_with_path() {
        let src = with("fullscreen = true", "fullscreen = true\nvsync = true");
        let issue = &issues_of(&src)[0];
        assert_eq!(issue.path, "display.vsync");
        assert!(issue.message.contains("unknown field `vsync`"), "{issue}");
    }

    #[test]
    fn toml_syntax_errors_have_a_location() {
        let issue = &issues_of("[game\nname = 1")[0];
        assert!(issue.path.is_empty());
        assert_eq!(issue.location.map(|l| l.line), Some(1));
    }

    #[test]
    fn every_validation_problem_is_reported_at_once() {
        let src = FULL
            .replace("\"Lighthouse Tale\"", "\"  \"")
            .replace("\"com.example.lighthouse\"", "\"com..bad\"")
            .replace("\"1.2.3\"", "\"1.2\"")
            .replace("\"^0.3.1\"", "\"newest\"")
            .replace("api     = 2", "api     = 0")
            .replace("width      = 320", "width      = 0")
            .replace("height     = 180", "height     = -1")
            .replace("scale      = 4", "scale      = 17")
            .replace("\"lighthouse_1f\"", "\"Lighthouse\"")
            .replace("[5, 7]", "[-1, 99999999999]")
            .replace("\"left\"", "\"north\"")
            .replace("\"scripts/main.lua\"", "\"../main.lua\"")
            .replace("\"day-night\"]", "\"Day\", \"resonance\"]")
            .replace("default   = \"it\"", "default   = \"fr\"")
            .replace("\"pt-BR\"]", "\"en\", \"x\"]")
            .replace("[\"z\", \"enter\"]", "[\"z\", \"\", \"z\"]")
            .replace("\"resonance:toggle\" = [\"r\"]", "\"Bad:Action\" = []");
        assert_eq!(
            paths_of(&src),
            [
                "game.name",
                "game.id",
                "game.version",
                "engine.version",
                "engine.api",
                "display.width",
                "display.height",
                "display.scale",
                "start.map",
                "start.position[0]",
                "start.position[1]",
                "start.facing",
                "start.script",
                "plugins.enabled[1]",
                "plugins.enabled[2]",
                "locale.available[3]",
                "locale.available[2]",
                "locale.available",
                "input.Bad:Action",
                "input.Bad:Action",
                "input.confirm[1]",
                "input.confirm[2]",
            ]
        );
    }

    #[test]
    fn error_messages_explain_the_problem() {
        let err = ProjectManifest::from_toml_str(&with("scale      = 4", "scale      = 0"))
            .expect_err("invalid");
        assert_eq!(
            err.to_string(),
            "pkrs.toml is invalid:\n  - `display.scale`: must be between 1 and 16 (got 0)"
        );
        let err =
            ProjectManifest::from_toml_str(&with("\"left\"", "\"north\"")).expect_err("invalid");
        assert!(
            err.to_string()
                .contains("`start.facing`: `north` is not one of up, down, left, right")
        );
    }

    #[test]
    fn game_id_rules() {
        assert!(check_game_id("com.example.lighthouse").is_ok());
        assert!(check_game_id("tale").is_ok());
        assert!(check_game_id("org.my-studio.Tale_2").is_ok());
        assert!(check_game_id("").is_err());
        assert!(check_game_id(".tale").is_err());
        assert!(check_game_id("com.ex ample").is_err());
        assert!(check_game_id("com/example").is_err());
    }

    #[test]
    fn script_path_rules() {
        assert!(check_script_path("scripts/main.lua").is_ok());
        assert!(check_script_path("main.lua").is_ok());
        assert!(check_script_path("scripts\\main.lua").is_err());
        assert!(check_script_path("/abs/main.lua").is_err());
        assert!(check_script_path("C:/main.lua").is_err());
        assert!(check_script_path("scripts/../main.lua").is_err());
        assert!(check_script_path("scripts//main.lua").is_err());
        assert!(check_script_path("scripts/main.py").is_err());
    }

    #[test]
    fn name_rules() {
        assert!(is_plugin_id("day-night"));
        assert!(is_plugin_id("resonance2"));
        assert!(!is_plugin_id("Day"));
        assert!(!is_plugin_id("-x"));
        assert!(is_language_tag("en"));
        assert!(is_language_tag("pt-BR"));
        assert!(is_language_tag("zh-Hant-TW"));
        assert!(!is_language_tag("e"));
        assert!(!is_language_tag("en_US"));
        assert!(!is_language_tag("en-"));
        assert!(is_action_name("confirm"));
        assert!(is_action_name("resonance:toggle"));
        assert!(is_action_name("day-night:skip_hour"));
        assert!(!is_action_name("Confirm"));
        assert!(!is_action_name("a:b:c"));
        assert!(!is_action_name(":toggle"));
    }

    #[test]
    fn load_reports_missing_file() {
        let dir = std::env::temp_dir().join("pkrs-core-manifest-missing-project");
        let err = ProjectManifest::load(&dir).expect_err("no manifest");
        assert!(matches!(err, ManifestError::Io { .. }), "{err:?}");
        assert!(err.to_string().contains("pkrs.toml"), "{err}");
        assert!(err.issues().is_empty());
    }

    #[test]
    fn manifest_error_converts_into_core_error() {
        fn load() -> crate::Result<ProjectManifest> {
            Ok(ProjectManifest::from_toml_str("")?)
        }
        assert!(matches!(load(), Err(crate::CoreError::Manifest(_))));
    }
}
