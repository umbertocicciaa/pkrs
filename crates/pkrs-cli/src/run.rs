//! `pkrs run`: find the project and the runtime binary, then launch it (§2.3, §14.1).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Environment variable that overrides the runtime binary used by `pkrs run`.
pub const RUNTIME_ENV: &str = "PKRS_RUNTIME";

/// File name of the runtime binary on this OS.
#[must_use]
pub fn runtime_file_name() -> String {
    format!("pkrs-runtime{}", std::env::consts::EXE_SUFFIX)
}

/// Why `pkrs run` could not start.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// No `pkrs.toml` in the directory or any parent.
    #[error("no pkrs.toml found in `{0}` or any parent directory (run `pkrs new <name>` first)")]
    NoProject(PathBuf),
    /// No runtime binary could be found.
    #[error(
        "pkrs-runtime not found (looked for {looked}); build it with `cargo build -p pkrs-runtime` or set {RUNTIME_ENV}"
    )]
    RuntimeNotFound {
        /// Where we looked.
        looked: String,
    },
    /// `--warp` was malformed.
    #[error("--warp expects `x,y` with non-negative integers (got `{0}`)")]
    BadWarp(String),
}

/// What to launch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunRequest {
    /// Project folder.
    pub project: PathBuf,
    /// `--map <id>`.
    pub map: Option<String>,
    /// `--warp x,y`.
    pub warp: Option<(u32, u32)>,
    /// `--headless` (forwarded; no window).
    pub headless: bool,
    /// `--ticks N` (forwarded).
    pub ticks: Option<u64>,
}

/// Parses `x,y`.
pub fn parse_warp(raw: &str) -> Result<(u32, u32), RunError> {
    let bad = || RunError::BadWarp(raw.to_owned());
    let (x, y) = raw.split_once(',').ok_or_else(bad)?;
    let x = x.trim().parse().map_err(|_| bad())?;
    let y = y.trim().parse().map_err(|_| bad())?;
    Ok((x, y))
}

/// The nearest directory at or above `start` that contains `pkrs.toml`.
pub fn find_project_root(start: &Path) -> Result<PathBuf, RunError> {
    start
        .ancestors()
        .find(|dir| dir.join("pkrs.toml").is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| RunError::NoProject(start.to_path_buf()))
}

/// Runtime binary: `$PKRS_RUNTIME` if set, else `pkrs-runtime` next to the `pkrs` executable.
/// (Downloading prebuilt runtimes per `engine.version` is M7, §2.5.)
pub fn locate_runtime(env_override: Option<PathBuf>, cli_exe: &Path) -> Result<PathBuf, RunError> {
    let mut looked = Vec::new();
    if let Some(path) = env_override {
        if path.is_file() {
            return Ok(path);
        }
        looked.push(format!("{RUNTIME_ENV}={}", path.display()));
    }
    if let Some(dir) = cli_exe.parent() {
        let sibling = dir.join(runtime_file_name());
        if sibling.is_file() {
            return Ok(sibling);
        }
        looked.push(sibling.display().to_string());
    }
    Err(RunError::RuntimeNotFound {
        looked: looked.join(", "),
    })
}

/// Runtime arguments for `request` (`--project <dir> [--map id] [--warp x,y] …`).
#[must_use]
pub fn runtime_args(request: &RunRequest) -> Vec<OsString> {
    let mut args: Vec<OsString> = vec!["--project".into(), request.project.clone().into()];
    if let Some(map) = &request.map {
        args.extend(["--map".into(), map.into()]);
    }
    if let Some((x, y)) = request.warp {
        args.extend(["--warp".into(), format!("{x},{y}").into()]);
    }
    if request.headless {
        args.push("--headless".into());
    }
    if let Some(ticks) = request.ticks {
        args.extend(["--ticks".into(), ticks.to_string().into()]);
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn warp_parsing() {
        assert_eq!(parse_warp("5,7").unwrap(), (5, 7));
        assert_eq!(parse_warp(" 1 , 2 ").unwrap(), (1, 2));
        for bad in ["5", "a,b", "-1,2", "1,2,3", ""] {
            assert!(parse_warp(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn runtime_arguments_are_forwarded() {
        let args = runtime_args(&RunRequest {
            project: PathBuf::from("demo"),
            map: Some("house".into()),
            warp: Some((3, 4)),
            headless: true,
            ticks: Some(60),
        });
        let args: Vec<_> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            [
                "--project",
                "demo",
                "--map",
                "house",
                "--warp",
                "3,4",
                "--headless",
                "--ticks",
                "60"
            ]
        );
        let minimal = runtime_args(&RunRequest {
            project: PathBuf::from("p"),
            ..RunRequest::default()
        });
        assert_eq!(minimal.len(), 2);
    }

    #[test]
    fn missing_runtime_reports_where_it_looked() {
        let err = locate_runtime(
            Some(PathBuf::from("does/not/exist")),
            Path::new("also/missing/pkrs"),
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("PKRS_RUNTIME=does"), "{msg}");
        assert!(msg.contains(&runtime_file_name()), "{msg}");
    }

    #[test]
    fn project_root_is_found_upwards() {
        let root = std::env::temp_dir().join(format!("pkrs-run-root-{}", std::process::id()));
        let nested = root.join("maps").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        std::fs::write(root.join("pkrs.toml"), "").unwrap();
        assert_eq!(find_project_root(&nested).unwrap(), root);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
