//! Plugin loading errors (§9.4): every problem names the plugin it is about.

use core::fmt;

use pkrs_core::{PluginManifestError, Version, VersionReq};
use thiserror::Error;

use crate::error::ScriptError;

/// One problem found while discovering, validating or ordering plugins.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginError {
    /// `plugins/<plugin>/plugin.toml` could not be read.
    #[error("plugin `{plugin}`: cannot read {path}: {reason}")]
    Read {
        /// Plugin folder name.
        plugin: String,
        /// Project-relative path of the manifest.
        path: String,
        /// Why it failed.
        reason: String,
    },

    /// `plugin.toml` failed to parse or validate.
    #[error("plugin `{plugin}`: {source}")]
    Manifest {
        /// Plugin folder name.
        plugin: String,
        /// The manifest problems.
        #[source]
        source: PluginManifestError,
    },

    /// `[plugin].id` differs from the folder name (`plugins/<id>/`).
    #[error(
        "plugin folder `plugins/{folder}` declares id `{id}`; the folder must be named after the id"
    )]
    IdMismatch {
        /// Folder name.
        folder: String,
        /// Declared id.
        id: String,
    },

    /// Two manifests declare the same id.
    #[error("plugin `{plugin}` is declared more than once")]
    Duplicate {
        /// The repeated id.
        plugin: String,
    },

    /// A plugin listed in `pkrs.toml [plugins].enabled` was not found in `plugins/`.
    #[error(
        "`plugins.enabled[{index}]`: plugin `{plugin}` is enabled but `plugins/{plugin}/plugin.toml` does not exist"
    )]
    NotFound {
        /// Missing plugin id.
        plugin: String,
        /// Its position in `enabled`.
        index: usize,
    },

    /// The plugin targets an API level this engine does not support (§9.7).
    #[error("plugin `{plugin}` targets API level {api}, but this engine supports {min}..={max}")]
    UnsupportedApi {
        /// Plugin id.
        plugin: String,
        /// Requested level.
        api: u32,
        /// Lowest supported level.
        min: u32,
        /// Highest supported level.
        max: u32,
    },

    /// A dependency is not installed at all.
    #[error("plugin `{plugin}` depends on `{dependency}` ({requirement}), which is not installed")]
    MissingDependency {
        /// Dependent plugin id.
        plugin: String,
        /// Missing dependency id.
        dependency: String,
        /// Required version range.
        requirement: VersionReq,
    },

    /// A dependency is installed but not listed in `pkrs.toml [plugins].enabled`.
    #[error(
        "plugin `{plugin}` depends on `{dependency}`, which is installed but not enabled in pkrs.toml"
    )]
    DependencyNotEnabled {
        /// Dependent plugin id.
        plugin: String,
        /// Disabled dependency id.
        dependency: String,
    },

    /// A dependency's version does not satisfy the requirement.
    #[error(
        "plugin `{plugin}` requires `{dependency}` {requirement}, but version {found} is installed"
    )]
    VersionMismatch {
        /// Dependent plugin id.
        plugin: String,
        /// Dependency id.
        dependency: String,
        /// Required version range.
        requirement: VersionReq,
        /// Installed version.
        found: Version,
    },

    /// Two enabled plugins conflict (`[load].conflicts`).
    #[error("plugin `{plugin}` conflicts with `{other}`; they cannot both be enabled")]
    Conflict {
        /// Plugin declaring the conflict.
        plugin: String,
        /// The conflicting plugin.
        other: String,
    },

    /// `dependencies` + `load.after` form a cycle. The list starts and ends with the same id;
    /// each plugin must load before the next.
    #[error("plugin load order has a cycle: {}", .cycle.join(" -> "))]
    Cycle {
        /// The cycle, e.g. `["a", "b", "a"]`.
        cycle: Vec<String>,
    },
}

/// Every problem found while resolving the plugin set; reported all at once.
#[derive(Debug)]
pub struct PluginErrors(pub Vec<PluginError>);

impl PluginErrors {
    /// The individual problems, in a deterministic order.
    #[must_use]
    pub fn errors(&self) -> &[PluginError] {
        &self.0
    }
}

impl fmt::Display for PluginErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("plugins cannot be loaded:")?;
        for err in &self.0 {
            write!(f, "\n  - {err}")?;
        }
        Ok(())
    }
}

impl std::error::Error for PluginErrors {}

/// Why an enabled, resolved plugin did not finish initialising. Failures are reported, never
/// fatal (§9.6); plugins that depend on a failed plugin are skipped.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PluginInitError {
    /// Binding the manifest or running `init.lua` failed.
    #[error("plugin `{plugin}` failed to initialise: {source}")]
    Script {
        /// Plugin id.
        plugin: String,
        /// The script error (with `file:line` when known).
        #[source]
        source: ScriptError,
    },
    /// A (transitive) dependency failed, so the plugin was not run.
    #[error("plugin `{plugin}` was skipped because its dependency `{dependency}` failed to load")]
    DependencyFailed {
        /// Plugin id.
        plugin: String,
        /// The failed dependency.
        dependency: String,
    },
}

impl PluginInitError {
    /// The plugin that failed.
    #[must_use]
    pub fn plugin(&self) -> &str {
        match self {
            Self::Script { plugin, .. } | Self::DependencyFailed { plugin, .. } => plugin,
        }
    }
}
