//! Loading a project's plugins at start-up (§9.4): resolve `pkrs.toml [plugins].enabled`
//! against `plugins/*/plugin.toml`, merge the `data/` folders into the content database, run
//! each `init.lua` in load order, and mount each plugin's `assets/` under `plugin_id:`.

use core::fmt;

use pkrs_assets::{Assets, AssetsError};
use pkrs_core::GameRng;
use pkrs_script::{
    ContentError, LuaScriptHost, PluginErrors, PluginLoadReport, PluginPlan, ScriptError,
    ScriptLimits, load_plugins, plan_plugins,
};

use crate::project::{Project, SUPPORTED_API};
use crate::scripts::VfsScriptFiles;

/// Every content problem found while merging `data/` folders (§8.2: reported all at once).
#[derive(Debug)]
pub struct ContentErrors(pub Vec<ContentError>);

impl fmt::Display for ContentErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("content cannot be loaded:")?;
        for err in &self.0 {
            write!(f, "\n  - {err}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ContentErrors {}

/// Why a project's scripts could not start.
#[derive(Debug, thiserror::Error)]
pub enum PluginStartError {
    /// The enabled plugin set is invalid (missing, cycle, conflict, version, API level).
    #[error(transparent)]
    Plugins(#[from] PluginErrors),
    /// The script host could not be created.
    #[error(transparent)]
    Script(#[from] ScriptError),
    /// The `data/` folders have errors, e.g. a duplicate id without `override` (§9.4).
    #[error(transparent)]
    Content(#[from] ContentErrors),
}

/// Resolves the project's enabled plugins into load order.
pub fn plan_project_plugins(project: &Project) -> Result<PluginPlan, PluginErrors> {
    let files = VfsScriptFiles::new(project.vfs.clone());
    plan_plugins(&files, &project.manifest.plugins.enabled, SUPPORTED_API)
}

/// Creates the project's sandboxed script host, merges content and loads its plugins in
/// resolved order.
///
/// An invalid plugin set or content errors in a `data/` folder are errors. A plugin whose
/// `init.lua` fails is reported in the returned [`PluginLoadReport`] and logged, but does not
/// stop the game (§9.6).
pub fn start_project_scripts(
    project: &Project,
    rng: Box<dyn GameRng>,
) -> Result<(LuaScriptHost, PluginLoadReport), PluginStartError> {
    let plan = plan_project_plugins(project)?;
    let files = VfsScriptFiles::new(project.vfs.clone());
    let mut host = LuaScriptHost::new(
        Box::new(VfsScriptFiles::new(project.vfs.clone())),
        rng,
        ScriptLimits::default(),
    )?;
    let mut report = load_plugins(&mut host, &files, &plan);
    if !report.content_errors.is_empty() {
        return Err(ContentErrors(std::mem::take(&mut report.content_errors)).into());
    }
    tracing::info!(
        loaded = report.loaded.len(),
        failed = report.failed.len(),
        content_files = report.content_files,
        content_overrides = report.content_overrides.len(),
        order = %report.loaded.join(", "),
        "plugins loaded"
    );
    Ok((host, report))
}

/// The project's [`Assets`] with every plugin in `plugins` that ships an `assets/` folder
/// mounted under its id (`resonance:ui/icon.png`, §9.4, §13).
pub fn project_assets(project: &Project, plugins: &[String]) -> Result<Assets, AssetsError> {
    let mut assets = Assets::new(project.vfs.clone());
    for id in plugins {
        if assets.mount_plugin_assets(id, project.vfs.clone())? {
            tracing::debug!(plugin = %id, "plugin assets mounted");
        }
    }
    Ok(assets)
}
