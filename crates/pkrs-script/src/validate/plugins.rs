//! Plugin problems (§9.4) as located [`Diagnostic`]s: manifest errors in `plugin.toml`,
//! plugin-graph errors in `pkrs.toml` or the dependent plugin's manifest, failed `init.lua`
//! runs at their `file:line`.

use pkrs_core::manifest::diagnostic::locate_toml_path;
use pkrs_core::{Diagnostic, ProjectManifest, SourceSpan};

use crate::error::ScriptError;
use crate::host::ScriptFiles;
use crate::plugin::{PluginError, PluginInitError, manifest_path};

const MANIFEST: &str = ProjectManifest::FILE_NAME;

/// `err` as diagnostics, with spans read from `files`.
#[must_use]
pub fn plugin_error_diagnostics(err: &PluginError, files: &dyn ScriptFiles) -> Vec<Diagnostic> {
    let read = |path: &str| files.read_script(path).ok();
    let in_toml = |file: &str, path: &str, d: Diagnostic| {
        let span = read(file).and_then(|src| locate_toml_path(&src, path));
        d.in_file(file).at(span)
    };
    let text = err.to_string();
    match err {
        PluginError::Read { path, .. } => {
            vec![Diagnostic::error("plugin::read", text).in_file(path.as_str())]
        }
        PluginError::Manifest { plugin, source } => {
            let file = manifest_path(plugin);
            source.diagnostics(&file, read(&file).as_deref())
        }
        PluginError::IdMismatch { folder, .. } => vec![in_toml(
            &manifest_path(folder),
            "plugin.id",
            Diagnostic::error("plugin::id-mismatch", text).with_label("must match the folder name"),
        )],
        PluginError::Duplicate { plugin } => {
            vec![in_toml(
                MANIFEST,
                "plugins.enabled",
                Diagnostic::error("plugin::duplicate", text)
                    .with_label(format!("`{plugin}` is declared more than once")),
            )]
        }
        PluginError::NotFound { plugin, index } => vec![in_toml(
            MANIFEST,
            &format!("plugins.enabled[{index}]"),
            Diagnostic::error("plugin::not-found", text)
                .with_label("plugin not found")
                .with_help(format!(
                    "create it with `pkrs plugin new {plugin}`, or remove it from the list"
                )),
        )],
        PluginError::UnsupportedApi { plugin, .. } => vec![in_toml(
            &manifest_path(plugin),
            "plugin.api",
            Diagnostic::error("plugin::unsupported-api", text).with_label("unsupported API level"),
        )],
        PluginError::MissingDependency {
            plugin, dependency, ..
        } => vec![in_toml(
            &manifest_path(plugin),
            &format!("dependencies.{dependency}"),
            Diagnostic::error("plugin::missing-dependency", text).with_label("not installed"),
        )],
        PluginError::DependencyNotEnabled { plugin, dependency } => vec![in_toml(
            &manifest_path(plugin),
            &format!("dependencies.{dependency}"),
            Diagnostic::error("plugin::dependency-not-enabled", text)
                .with_label("not enabled")
                .with_help(format!(
                    "add \"{dependency}\" to `[plugins] enabled` in pkrs.toml"
                )),
        )],
        PluginError::VersionMismatch {
            plugin, dependency, ..
        } => vec![in_toml(
            &manifest_path(plugin),
            &format!("dependencies.{dependency}"),
            Diagnostic::error("plugin::version-mismatch", text).with_label("requirement not met"),
        )],
        PluginError::Conflict { plugin, other } => {
            let file = manifest_path(plugin);
            let span =
                read(&file).and_then(|src| SourceSpan::find(&src, &format!("\"{other}\""), 0));
            vec![
                Diagnostic::error("plugin::conflict", text)
                    .in_file(file)
                    .at(span)
                    .with_label("conflicting plugin"),
            ]
        }
        PluginError::Cycle { .. } => vec![in_toml(
            MANIFEST,
            "plugins.enabled",
            Diagnostic::error("plugin::cycle", text)
                .with_label("these plugins cannot be ordered")
                .with_help(
                    "remove a `dependencies` entry or a `[load] after` hint to break the cycle",
                ),
        )],
    }
}

/// A plugin whose `init.lua` failed (`pkrs check --plugins`), at the failing line.
#[must_use]
pub fn plugin_init_diagnostic(err: &PluginInitError) -> Diagnostic {
    let diagnostic = Diagnostic::error("plugin::init", err.to_string());
    match err {
        PluginInitError::Script { source, .. } => match source {
            ScriptError::Located {
                file,
                line,
                message,
                ..
            } => Diagnostic::error(
                "plugin::init",
                format!("plugin `{}` failed to initialise: {message}", err.plugin()),
            )
            .in_file(file.as_str())
            .at(Some(SourceSpan::new(*line as usize, 1, 0)))
            .with_label(message.clone()),
            _ => diagnostic,
        },
        PluginInitError::DependencyFailed { .. } => diagnostic,
    }
}
