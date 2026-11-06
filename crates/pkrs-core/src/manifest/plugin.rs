//! The plugin manifest, `plugins/<id>/plugin.toml` (§9.4).
//!
//! ```toml
//! [plugin]
//! id          = "resonance"
//! name        = "Resonance"
//! version     = "1.2.0"
//! api         = 1                 # scripting API level required (§9.7)
//! description = "Once per battle, a creature can Resonate."
//!
//! [dependencies]
//! "status-pack" = "^1.0"          # other plugins, semver ranges
//!
//! [load]
//! after       = ["day-night"]     # soft ordering hints (no dependency)
//! conflicts   = ["overdrive"]
//! ```
//!
//! Like `pkrs.toml`, loading is parse (types, unknown keys) then validate (every problem at
//! once, with dotted field paths). Only `[plugin]` `id`, `name`, `version` and `api` are
//! required. Cross-plugin rules (missing dependencies, version mismatches, conflicts, cycles,
//! supported API levels) need the whole plugin set and are checked by the plugin loader in
//! `pkrs-script`.

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;

use super::error::PluginManifestError;
use super::{Validator, is_plugin_id, toml_codec};
use crate::version::{Version, VersionReq};

/// The parsed and validated `plugin.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginManifest {
    /// `[plugin]`: identity of the plugin.
    pub plugin: PluginInfo,
    /// `[dependencies]`: plugin id → semver requirement, sorted by id. Dependencies must be
    /// enabled too, and are always loaded first.
    pub dependencies: BTreeMap<String, VersionReq>,
    /// `[load]`: soft ordering hints and conflicts.
    pub load: PluginLoadRules,
}

/// `[plugin]` table.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginInfo {
    /// Plugin id (lowercase letters, digits, `_`, `-`); must match the plugin's folder name.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Plugin version, checked against dependants' requirements.
    pub version: Version,
    /// Scripting API level the plugin targets (§9.7).
    pub api: u32,
    /// Optional one-line description (default empty).
    pub description: String,
}

/// `[load]` table.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PluginLoadRules {
    /// Plugins this one should load after *if they are enabled* (no dependency).
    pub after: Vec<String>,
    /// Plugins that must not be enabled together with this one.
    pub conflicts: Vec<String>,
}

impl PluginManifest {
    /// File name of the manifest inside a plugin folder.
    pub const FILE_NAME: &'static str = "plugin.toml";

    /// Parses and validates `plugin.toml` source text.
    pub fn from_toml_str(src: &str) -> Result<Self, PluginManifestError> {
        let raw: RawPluginManifest = toml_codec::parse(src).map_err(PluginManifestError::Parse)?;
        validate(raw)
    }

    /// The plugin id (`[plugin].id`).
    #[must_use]
    pub fn id(&self) -> &str {
        &self.plugin.id
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluginManifest {
    plugin: RawPluginInfo,
    #[serde(default)]
    dependencies: BTreeMap<String, String>,
    #[serde(default)]
    load: RawLoad,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPluginInfo {
    id: String,
    name: String,
    version: String,
    api: i64,
    #[serde(default)]
    description: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawLoad {
    after: Vec<String>,
    conflicts: Vec<String>,
}

fn validate(raw: RawPluginManifest) -> Result<PluginManifest, PluginManifestError> {
    let mut v = Validator::default();
    let id = raw.plugin.id;
    let id_ok = is_plugin_id(&id);
    if !id_ok {
        v.push("plugin.id", not_a_plugin_id(&id));
    }
    v.non_empty("plugin.name", &raw.plugin.name);
    let version = Version::parse(&raw.plugin.version).unwrap_or_else(|e| {
        v.push("plugin.version", e.to_string());
        Version::new(0, 0, 0)
    });
    let api = v.ranged_u32("plugin.api", raw.plugin.api, 1..=u32::MAX);

    let mut dependencies = BTreeMap::new();
    for (dep, req) in raw.dependencies {
        let path = format!("dependencies.{dep}");
        if !is_plugin_id(&dep) {
            v.push(path.as_str(), not_a_plugin_id(&dep));
        } else if id_ok && dep == id {
            v.push(path.as_str(), "a plugin cannot depend on itself");
        }
        match VersionReq::parse(&req) {
            Ok(req) => {
                dependencies.insert(dep, req);
            }
            Err(_) => v.push(
                path.as_str(),
                format!("`{req}` is not a semver requirement (e.g. \"1.0\" or \"^1.2\")"),
            ),
        }
    }

    let id_list = |v: &mut Validator, field: &str, items: &[String]| {
        for (i, item) in items.iter().enumerate() {
            let path = format!("load.{field}[{i}]");
            if !is_plugin_id(item) {
                v.push(path, not_a_plugin_id(item));
            } else if id_ok && *item == id {
                v.push(path, "a plugin cannot name itself");
            }
        }
        v.unique(&format!("load.{field}"), items);
    };
    id_list(&mut v, "after", &raw.load.after);
    id_list(&mut v, "conflicts", &raw.load.conflicts);
    let deps: BTreeSet<&str> = dependencies.keys().map(String::as_str).collect();
    for (i, other) in raw.load.conflicts.iter().enumerate() {
        if deps.contains(other.as_str()) {
            v.push(
                format!("load.conflicts[{i}]"),
                format!("`{other}` is also listed in [dependencies]"),
            );
        }
    }

    if v.issues.is_empty() {
        Ok(PluginManifest {
            plugin: PluginInfo {
                id,
                name: raw.plugin.name,
                version,
                api,
                description: raw.plugin.description,
            },
            dependencies,
            load: PluginLoadRules {
                after: raw.load.after,
                conflicts: raw.load.conflicts,
            },
        })
    } else {
        Err(PluginManifestError::Invalid(v.issues))
    }
}

fn not_a_plugin_id(id: &str) -> String {
    format!(
        "`{id}` is not a valid plugin id (lowercase letter first, then lowercase letters, digits, `_` or `-`)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"
[plugin]
id          = "resonance"
name        = "Resonance"
version     = "1.2.0"
api         = 1
description = "Once per battle, a creature can Resonate."

[dependencies]
"status-pack" = "^1.0"

[load]
after     = ["day-night"]
conflicts = ["overdrive"]
"#;

    fn paths(err: &PluginManifestError) -> Vec<&str> {
        err.issues().iter().map(|i| i.path.as_str()).collect()
    }

    #[test]
    fn parses_the_design_example() {
        let m = PluginManifest::from_toml_str(FULL).expect("valid");
        assert_eq!(m.id(), "resonance");
        assert_eq!(m.plugin.name, "Resonance");
        assert_eq!(m.plugin.version, Version::new(1, 2, 0));
        assert_eq!(m.plugin.api, 1);
        let req = &m.dependencies["status-pack"];
        assert!(req.matches(&Version::new(1, 9, 0)));
        assert!(!req.matches(&Version::new(2, 0, 0)));
        assert_eq!(m.load.after, ["day-night"]);
        assert_eq!(m.load.conflicts, ["overdrive"]);
    }

    #[test]
    fn minimal_manifest_has_defaults() {
        let m = PluginManifest::from_toml_str(
            "[plugin]\nid = \"a\"\nname = \"A\"\nversion = \"0.1.0\"\napi = 1\n",
        )
        .expect("valid");
        assert!(m.dependencies.is_empty());
        assert_eq!(m.load, PluginLoadRules::default());
        assert_eq!(m.plugin.description, "");
    }

    #[test]
    fn unknown_keys_and_missing_fields_are_parse_errors() {
        let err = PluginManifest::from_toml_str(
            "[plugin]\nid = \"a\"\nname = \"A\"\nversion = \"0.1.0\"\napi = 1\nauthor = \"x\"\n",
        )
        .expect_err("unknown key");
        assert!(matches!(err, PluginManifestError::Parse(_)), "{err}");
        assert!(err.to_string().starts_with("plugin.toml: "), "{err}");

        let err = PluginManifest::from_toml_str("[plugin]\nid = \"a\"\n").expect_err("missing");
        assert!(matches!(err, PluginManifestError::Parse(_)), "{err}");
    }

    #[test]
    fn reports_every_validation_problem() {
        let err = PluginManifest::from_toml_str(
            r#"
[plugin]
id = "Bad Id"
name = " "
version = "1.0"
api = 0

[dependencies]
"Nope" = "latest"

[load]
after = ["x", "x"]
conflicts = ["y"]
"#,
        )
        .expect_err("invalid");
        assert_eq!(
            paths(&err),
            [
                "plugin.id",
                "plugin.name",
                "plugin.version",
                "plugin.api",
                "dependencies.Nope",
                "dependencies.Nope",
                "load.after[1]",
            ]
        );
    }

    #[test]
    fn self_references_and_dependency_conflicts_are_rejected() {
        let err = PluginManifest::from_toml_str(
            r#"
[plugin]
id = "a"
name = "A"
version = "1.0.0"
api = 1

[dependencies]
a = "1"
b = "1"

[load]
after = ["a"]
conflicts = ["a", "b"]
"#,
        )
        .expect_err("invalid");
        assert_eq!(
            paths(&err),
            [
                "dependencies.a",
                "load.after[0]",
                "load.conflicts[0]",
                "load.conflicts[0]",
                "load.conflicts[1]",
            ]
        );
        assert!(err.to_string().starts_with("plugin.toml is invalid:"));
    }
}
