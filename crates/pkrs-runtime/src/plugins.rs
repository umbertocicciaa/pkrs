//! Loading a project's plugins at start-up (§9.4): resolve `pkrs.toml [plugins].enabled`
//! against `plugins/*/plugin.toml`, then run each `init.lua` in load order.

use pkrs_core::GameRng;
use pkrs_script::{
    LuaScriptHost, PluginErrors, PluginLoadReport, PluginPlan, ScriptError, ScriptLimits,
    load_plugins, plan_plugins,
};

use crate::project::{Project, SUPPORTED_API};
use crate::scripts::VfsScriptFiles;

/// Why a project's scripts could not start.
#[derive(Debug, thiserror::Error)]
pub enum PluginStartError {
    /// The enabled plugin set is invalid (missing, cycle, conflict, version, API level).
    #[error(transparent)]
    Plugins(#[from] PluginErrors),
    /// The script host could not be created.
    #[error(transparent)]
    Script(#[from] ScriptError),
}

/// Resolves the project's enabled plugins into load order.
pub fn plan_project_plugins(project: &Project) -> Result<PluginPlan, PluginErrors> {
    let files = VfsScriptFiles::new(project.vfs.clone());
    plan_plugins(&files, &project.manifest.plugins.enabled, SUPPORTED_API)
}

/// Creates the project's sandboxed script host and loads its plugins in resolved order.
///
/// An invalid plugin set is an error. A plugin whose `init.lua` fails is reported in the
/// returned [`PluginLoadReport`] and logged, but does not stop the game (§9.6).
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
    let report = load_plugins(&mut host, &files, &plan);
    tracing::info!(
        loaded = report.loaded.len(),
        failed = report.failed.len(),
        order = %report.loaded.join(", "),
        "plugins loaded"
    );
    Ok((host, report))
}
