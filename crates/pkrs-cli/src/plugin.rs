//! `pkrs plugin new` (§2.3, §9.4): scaffolds `plugins/<id>/` from an embedded template.
//!
//! The template lives under `crates/pkrs-cli/templates/plugin/` and is embedded with
//! `include_dir`, like the project templates. Its text files have `{{id}}`, `{{name}}` and
//! `{{api}}` substituted. It holds a `plugin.toml`, an `init.lua` with battle-hook and
//! world-event examples, and a `README.md`; it loads without edits once the plugin is listed
//! in `[plugins].enabled`.
//!
//! `pkrs.toml` is never edited here (only `pkrs upgrade` edits it, §2.5); [`PluginScaffold`]
//! says whether the plugin still needs enabling so the CLI can print the line to add.

use std::fmt;
use std::path::{Path, PathBuf};

use include_dir::{Dir, include_dir};
use pkrs_core::{ProjectManifest, is_plugin_id};

use crate::check::SUPPORTED_API;
use crate::template::{TemplateError, slug, valid_name, write_tree};

static PLUGIN: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/templates/plugin");

/// Values substituted into the plugin template.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginVars {
    /// `plugin.id`, also the folder name under `plugins/`.
    pub id: String,
    /// `plugin.name` (the name given to `pkrs plugin new`).
    pub name: String,
    /// `plugin.api`: the newest scripting API level this CLI supports (§9.7).
    pub api: u32,
}

impl PluginVars {
    /// Variables for a plugin called `name`; its id is the slug of `name` (`Day Night` →
    /// `day-night`), which must be a valid plugin id.
    pub fn for_plugin(name: &str) -> Result<Self, TemplateError> {
        let id = slug(name);
        if !valid_name(name) || !is_plugin_id(&id) {
            return Err(TemplateError::InvalidPluginName(name.to_owned()));
        }
        Ok(Self {
            id,
            name: name.to_owned(),
            api: *SUPPORTED_API.end(),
        })
    }
}

impl fmt::Display for PluginVars {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin `{}` ({}, api {})", self.id, self.name, self.api)
    }
}

/// Substitutes `{{id}}`, `{{name}}` and `{{api}}`.
#[must_use]
pub fn render_plugin(text: &str, vars: &PluginVars) -> String {
    text.replace("{{id}}", &vars.id)
        .replace("{{name}}", &vars.name)
        .replace("{{api}}", &vars.api.to_string())
}

/// What `pkrs plugin new` wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginScaffold {
    /// The new plugin folder (`<project>/plugins/<id>`).
    pub dir: PathBuf,
    /// Files written, relative to [`dir`](Self::dir), in a stable order.
    pub files: Vec<PathBuf>,
    /// The `pkrs.toml` lines that enable the plugin ([`enable_hint`]), or `None` when
    /// `[plugins].enabled` already lists it. If the manifest cannot be read the hint lists
    /// only this plugin (`pkrs check` reports the manifest problem).
    pub enable_hint: Option<String>,
}

/// Writes the plugin template into `<project>/plugins/<id>/` (created if missing; must be
/// empty if it exists).
pub fn new_plugin(project: &Path, vars: &PluginVars) -> Result<PluginScaffold, TemplateError> {
    let dir = project.join("plugins").join(&vars.id);
    let files = write_tree(&PLUGIN, &dir, |text| render_plugin(text, vars))?;
    let enabled = ProjectManifest::load(project)
        .map(|m| m.plugins.enabled)
        .unwrap_or_default();
    let hint = (!enabled.contains(&vars.id)).then(|| enable_hint(&enabled, &vars.id));
    Ok(PluginScaffold {
        dir,
        files,
        enable_hint: hint,
    })
}

/// The `pkrs.toml` line that enables plugin `id` alongside the already `enabled` ones.
#[must_use]
pub fn enable_hint(enabled: &[String], id: &str) -> String {
    let ids: Vec<String> = enabled
        .iter()
        .map(String::as_str)
        .chain((!enabled.iter().any(|e| e == id)).then_some(id))
        .map(|e| format!("\"{e}\""))
        .collect();
    format!("[plugins]\nenabled = [{}]", ids.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_core::PluginManifest;

    fn vars() -> PluginVars {
        PluginVars::for_plugin("Day Night").unwrap()
    }

    fn template_text(path: &str) -> &'static str {
        PLUGIN
            .get_file(path)
            .and_then(|f| f.contents_utf8())
            .unwrap()
    }

    #[test]
    fn names_become_plugin_ids() {
        let v = vars();
        assert_eq!(v.id, "day-night");
        assert_eq!(v.name, "Day Night");
        assert_eq!(v.api, *SUPPORTED_API.end());
        assert_eq!(PluginVars::for_plugin("resonance").unwrap().id, "resonance");
        for bad in ["", "2fast", "_x", "a/b", "../x", "dots.no", "trail ", "é"] {
            assert!(
                matches!(
                    PluginVars::for_plugin(bad),
                    Err(TemplateError::InvalidPluginName(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn render_substitutes_every_placeholder() {
        let v = PluginVars {
            id: "x".into(),
            name: "X".into(),
            api: 7,
        };
        assert_eq!(
            render_plugin("{{id}}|{{name}}|{{api}}|{{id}}", &v),
            "x|X|7|x"
        );
    }

    #[test]
    fn rendered_manifest_is_valid() {
        let rendered = render_plugin(template_text("plugin.toml"), &vars());
        let manifest = PluginManifest::from_toml_str(&rendered).unwrap();
        assert_eq!(manifest.id(), "day-night");
        assert_eq!(manifest.plugin.name, "Day Night");
        assert_eq!(manifest.plugin.api, *SUPPORTED_API.end());
        assert!(SUPPORTED_API.contains(&manifest.plugin.api));
    }

    #[test]
    fn template_has_manifest_init_and_hook_examples() {
        for path in ["plugin.toml", "init.lua", "README.md"] {
            assert!(PLUGIN.get_file(path).is_some(), "{path}");
        }
        let init = render_plugin(template_text("init.lua"), &vars());
        assert!(init.contains("plugin.define()"));
        assert!(init.contains("battle.on(\"on_damage_calc\""));
        assert!(init.contains("events.on(\"map_enter\""));
        for file in PLUGIN.files() {
            if let Some(text) = file.contents_utf8() {
                let out = render_plugin(text, &vars());
                assert!(!out.contains("{{"), "{:?} keeps a placeholder", file.path());
            }
        }
    }

    #[test]
    fn enable_hint_appends_once() {
        assert_eq!(
            enable_hint(&[], "day-night"),
            "[plugins]\nenabled = [\"day-night\"]"
        );
        let enabled = vec!["a".to_owned(), "day-night".to_owned()];
        assert_eq!(
            enable_hint(&enabled, "day-night"),
            "[plugins]\nenabled = [\"a\", \"day-night\"]"
        );
        assert_eq!(
            enable_hint(&enabled[..1], "b"),
            "[plugins]\nenabled = [\"a\", \"b\"]"
        );
    }

    #[test]
    fn scaffolds_into_the_plugins_folder_once() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let project =
            std::env::temp_dir().join(format!("pkrs-plugin-new-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&project).unwrap();
        let out = new_plugin(&project, &vars()).unwrap();
        assert_eq!(out.dir, project.join("plugins/day-night"));
        assert_eq!(
            out.files,
            [
                PathBuf::from("README.md"),
                PathBuf::from("init.lua"),
                PathBuf::from("plugin.toml")
            ]
        );
        assert_eq!(
            out.enable_hint.as_deref(),
            Some("[plugins]\nenabled = [\"day-night\"]"),
            "no pkrs.toml, so not enabled"
        );
        assert!(matches!(
            new_plugin(&project, &vars()),
            Err(TemplateError::DestinationNotEmpty(_))
        ));
        std::fs::remove_dir_all(&project).unwrap();
    }
}
