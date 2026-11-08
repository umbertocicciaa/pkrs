//! Start-up validation (§8.2): the runtime runs the same checks as `pkrs check`
//! ([`pkrs_script::validate::validate_project`]) before the game starts.
//!
//! - **Dev builds** log every diagnostic (errors at `error`, warnings at `warn`, target
//!   `content`) in the plain one-block form and start anyway, so a half-finished project can
//!   still be played.
//! - **Release builds** with errors write the report to `<logs>/content-errors.log` and refuse
//!   to start with an error naming that file, instead of crashing later on bad content.

use std::path::{Path, PathBuf};

use pkrs_assets::VfsPath;
use pkrs_core::{Diagnostics, Version};
use pkrs_platform::UserPaths;
use pkrs_script::validate::{ValidateOptions, validate_project};
use pkrs_world::{Map, MapError, MapImporter, TiledImporter};

use crate::project::{Project, SUPPORTED_API};
use crate::scripts::VfsScriptFiles;

/// File the release report is written to, inside the logs folder.
pub const REPORT_FILE: &str = "content-errors.log";

/// Release builds refused to start: the content has errors.
#[derive(Debug, thiserror::Error)]
#[error("the game content has {errors} error(s); see {}", report.display())]
pub struct InvalidContent {
    /// Number of errors.
    pub errors: usize,
    /// Where the report was written.
    pub report: PathBuf,
}

/// Validates `project` like `pkrs check` (without running plugin scripts).
#[must_use]
pub fn validate_project_content(project: &Project) -> Diagnostics {
    let files = VfsScriptFiles::new(project.vfs.clone());
    let vfs = project.vfs.clone();
    let mut import = |path: &str| -> Result<Map, MapError> {
        let vfs_path = VfsPath::new(path).map_err(|e| MapError::Parse {
            path: path.to_owned(),
            message: e.to_string(),
        })?;
        TiledImporter::new().import(&vfs, &vfs_path)
    };
    let Ok(engine) = Version::parse(env!("CARGO_PKG_VERSION")) else {
        return Diagnostics::new();
    };
    let options = ValidateOptions {
        engine,
        supported_api: SUPPORTED_API,
        plugin_content: None,
    };
    validate_project(&files, &mut import, &options).diagnostics
}

/// Logs `diagnostics`; with `release` and any error, also writes the report to
/// `<logs_dir>/content-errors.log` and returns [`InvalidContent`].
pub fn report_startup(
    diagnostics: &Diagnostics,
    logs_dir: &Path,
    release: bool,
) -> Result<(), InvalidContent> {
    for d in diagnostics {
        if d.is_error() {
            tracing::error!(target: "content", "{d}");
        } else {
            tracing::warn!(target: "content", "{d}");
        }
    }
    if !diagnostics.is_empty() {
        tracing::info!(
            target: "content",
            errors = diagnostics.error_count(),
            warnings = diagnostics.warning_count(),
            "content validated (run `pkrs check` for source snippets)"
        );
    }
    if !release || !diagnostics.has_errors() {
        return Ok(());
    }
    let report = logs_dir.join(REPORT_FILE);
    let written = std::fs::create_dir_all(logs_dir)
        .and_then(|()| std::fs::write(&report, format!("{diagnostics}\n")));
    if let Err(err) = written {
        tracing::error!(target: "content", path = %report.display(), %err, "cannot write the content report");
    }
    Err(InvalidContent {
        errors: diagnostics.error_count(),
        report,
    })
}

/// Validates `project` at start-up ([`validate_project_content`] + [`report_startup`]);
/// release behaviour follows `cfg!(debug_assertions)`.
pub fn startup_check(project: &Project) -> Result<(), InvalidContent> {
    let diagnostics = validate_project_content(project);
    report_startup(
        &diagnostics,
        project.paths.logs_dir(),
        !cfg!(debug_assertions),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use pkrs_core::Diagnostic;

    use super::*;

    #[test]
    fn dev_builds_start_with_errors_and_release_builds_write_a_report() {
        let diagnostics: Diagnostics =
            [Diagnostic::error("data::parse", "bad").in_file("data/a.ron")]
                .into_iter()
                .collect();
        let dir = std::env::temp_dir().join(format!("pkrs-startup-{}", std::process::id()));
        assert!(report_startup(&diagnostics, &dir, false).is_ok());
        let err = report_startup(&diagnostics, &dir, true).unwrap_err();
        assert_eq!(err.errors, 1);
        let text = std::fs::read_to_string(&err.report).unwrap();
        assert!(
            text.starts_with("error[data::parse]: bad\n  --> data/a.ron"),
            "{text}"
        );
        let warnings: Diagnostics = [Diagnostic::warning("script::tick-listener", "tick")]
            .into_iter()
            .collect();
        assert!(report_startup(&warnings, &dir, true).is_ok());
        std::fs::remove_dir_all(dir).ok();
    }
}
