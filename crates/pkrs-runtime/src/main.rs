//! The pkrs game player (§0.5): wires every subsystem, owns the state stack and main loop.

use std::path::PathBuf;

use anyhow::Context as _;
use pkrs_core::GridPos;
use pkrs_platform::{
    DisplaySettings, FixedPaths, MacroquadPlatform, NullPlatform, Platform, PlatformConfig,
};
use pkrs_render::{MacroquadRenderer, Renderer, RendererConfig};
use pkrs_runtime::project::{Project, ProjectHost, ProjectOptions};
use pkrs_runtime::wiring::{headless_platform, headless_renderer, window_config};
use pkrs_runtime::{
    AdapterSet, AnimationDemoHost, LoopExit, PlatformAdapter, RenderAdapter, RunMode,
    RuntimeLoopConfig, block_on_loop, run_loop,
};
use tracing_subscriber::EnvFilter;

/// Log filter used when `RUST_LOG` is not set.
const DEFAULT_LOG_FILTER: &str = "info";

/// Default `--ticks` when running headless without an explicit limit (avoids a busy spin).
const DEFAULT_HEADLESS_TICKS: u64 = 600;

fn main() -> anyhow::Result<()> {
    init_tracing()?;
    let cli = RuntimeCliOptions::parse()?;
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        headless = cli.headless,
        ticks = ?cli.ticks,
        vsync = cli.vsync,
        "pkrs-runtime starting"
    );

    let max_ticks = if cli.headless {
        Some(cli.ticks.unwrap_or(DEFAULT_HEADLESS_TICKS))
    } else {
        cli.ticks
    };

    let config = RuntimeLoopConfig {
        max_ticks,
        max_frames: None,
        vsync: cli.vsync,
    };

    let adapters = AdapterSet::for_mode(RunMode::from_headless_flag(cli.headless));
    match (adapters.platform, adapters.renderer, cli.project_options()) {
        (PlatformAdapter::Null, RenderAdapter::Null, Some(opts)) => {
            run_project_headless(&opts, config)
        }
        (PlatformAdapter::Macroquad, RenderAdapter::Macroquad, Some(opts)) => {
            run_project_windowed(&opts, config)
        }
        (PlatformAdapter::Null, RenderAdapter::Null, None) => run_headless(config),
        (PlatformAdapter::Macroquad, RenderAdapter::Macroquad, None) => run_windowed(config),
        (platform, renderer, _) => {
            anyhow::bail!("unsupported adapter combination: {platform:?} + {renderer:?}")
        }
    }
}

/// `--project` with Null adapters (CI, `pkrs run --headless`).
fn run_project_headless(opts: &ProjectOptions, config: RuntimeLoopConfig) -> anyhow::Result<()> {
    let project = Project::open(opts)
        .with_context(|| format!("cannot open project `{}`", opts.dir.display()))?;
    let (renderer, _log) = headless_renderer(project.display());
    let mut host = ProjectHost::new(&project, renderer).context("cannot start the project")?;
    let mut platform = Platform::new(
        Box::new(NullPlatform::new()),
        Box::new(project.paths.clone()),
        PlatformConfig::default(),
    );
    let exit = block_on_loop(&mut platform, &mut host, &config);
    tracing::info!(?exit, ticks = host.ticks(), "headless project run finished");
    check_headless_exit(exit, &config)
}

/// `--project` in a window (`pkrs run`).
fn run_project_windowed(opts: &ProjectOptions, config: RuntimeLoopConfig) -> anyhow::Result<()> {
    let project = Project::open(opts)
        .with_context(|| format!("cannot open project `{}`", opts.dir.display()))?;
    let display = project.display();
    let window = window_config(&project.manifest.game.name, &display, config.vsync);
    MacroquadPlatform::run(&window, move |backend| async move {
        let mut platform = Platform::new(
            Box::new(backend),
            Box::new(project.paths.clone()),
            PlatformConfig::default(),
        );
        let renderer = Renderer::new(
            Box::new(MacroquadRenderer::new()),
            display,
            RendererConfig::default(),
        );
        let mut host = match ProjectHost::new(&project, renderer) {
            Ok(host) => host,
            Err(err) => {
                tracing::error!(%err, "cannot start the project");
                return;
            }
        };
        let exit = run_loop(&mut platform, &mut host, &config).await;
        tracing::info!(?exit, "project run finished");
    });
    Ok(())
}

/// Runs with Null adapters only (§14.4 smoke test). Errors and early exits are fatal so CI
/// sees a non-zero status.
fn run_headless(config: RuntimeLoopConfig) -> anyhow::Result<()> {
    let display = DisplaySettings::default();
    let mut platform = headless_platform(PlatformConfig::default());
    let (renderer, log) = headless_renderer(display);
    let mut host = AnimationDemoHost::with_renderer(renderer).context("animation demo host")?;
    let exit = block_on_loop(&mut platform, &mut host, &config);
    tracing::info!(
        ?exit,
        frames = platform.frame_count(),
        draws = log.draws().len(),
        "headless run finished"
    );
    check_headless_exit(exit, &config)
}

/// A headless run with a tick limit must stop because of that limit (or a clean `Quit`).
fn check_headless_exit(exit: LoopExit, config: &RuntimeLoopConfig) -> anyhow::Result<()> {
    match exit {
        LoopExit::TickLimit | LoopExit::Quit => Ok(()),
        LoopExit::FrameLimit if config.max_frames.is_some() => Ok(()),
        LoopExit::FrameLimit => anyhow::bail!("headless run stopped unexpectedly: {exit:?}"),
    }
}

fn run_windowed(config: RuntimeLoopConfig) -> anyhow::Result<()> {
    let display = DisplaySettings::default();
    let window = window_config("pkrs", &display, config.vsync);
    MacroquadPlatform::run(&window, move |backend| async move {
        let mut platform = Platform::new(
            Box::new(backend),
            Box::new(FixedPaths::under("pkrs")),
            PlatformConfig::default(),
        );
        let mut host = match AnimationDemoHost::new(MacroquadRenderer::new()) {
            Ok(host) => host,
            Err(err) => {
                tracing::error!(%err, "failed to create animation demo host");
                return;
            }
        };
        let exit = run_loop(&mut platform, &mut host, &config).await;
        tracing::info!(
            ?exit,
            frames = platform.frame_count(),
            "windowed run finished"
        );
    });
    Ok(())
}

/// Parsed `pkrs-runtime` command-line flags (§2.3, §14.1, §15.2).
#[derive(Clone, Debug, PartialEq, Eq)]
struct RuntimeCliOptions {
    headless: bool,
    ticks: Option<u64>,
    vsync: bool,
    project: Option<PathBuf>,
    map: Option<String>,
    warp: Option<GridPos>,
}

/// Parses `x,y` (non-negative tile coordinates).
fn parse_warp(raw: &str) -> anyhow::Result<GridPos> {
    let (x, y) = raw
        .split_once(',')
        .context("--warp expects `x,y` (e.g. --warp 5,7)")?;
    let coord = |v: &str| -> anyhow::Result<i32> {
        let n: i32 = v
            .trim()
            .parse()
            .context("--warp coordinates must be integers")?;
        anyhow::ensure!(n >= 0, "--warp coordinates must be non-negative");
        Ok(n)
    };
    Ok(GridPos::new(coord(x)?, coord(y)?))
}

impl RuntimeCliOptions {
    fn project_options(&self) -> Option<ProjectOptions> {
        self.project.as_ref().map(|dir| ProjectOptions {
            dir: dir.clone(),
            map: self.map.clone(),
            warp: self.warp,
        })
    }
}

impl RuntimeCliOptions {
    fn parse() -> anyhow::Result<Self> {
        Self::parse_from(std::env::args().skip(1))
    }

    fn parse_from(args: impl IntoIterator<Item = impl AsRef<str>>) -> anyhow::Result<Self> {
        let mut headless = false;
        let mut ticks = None;
        let mut vsync = true;
        let mut project = None;
        let mut map = None;
        let mut warp = None;
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_ref() {
                "--headless" => headless = true,
                "--project" => {
                    let dir = args.next().context("--project requires a directory")?;
                    project = Some(PathBuf::from(dir.as_ref()));
                }
                "--map" => {
                    let id = args.next().context("--map requires a map id")?;
                    map = Some(id.as_ref().to_owned());
                }
                "--warp" => {
                    let raw = args.next().context("--warp requires `x,y`")?;
                    warp = Some(parse_warp(raw.as_ref())?);
                }
                "--no-vsync" => vsync = false,
                "--ticks" => {
                    let value = args
                        .next()
                        .context("--ticks requires a non-negative integer")?;
                    ticks = Some(
                        value
                            .as_ref()
                            .parse::<u64>()
                            .context("--ticks must be a non-negative integer")?,
                    );
                }
                "--help" | "-h" => {
                    print_help();
                    std::process::exit(0);
                }
                other => {
                    anyhow::bail!("unknown argument: {other} (try --help)");
                }
            }
        }
        anyhow::ensure!(
            project.is_some() || (map.is_none() && warp.is_none()),
            "--map and --warp need --project <dir>"
        );
        Ok(Self {
            headless,
            ticks,
            vsync,
            project,
            map,
            warp,
        })
    }
}

fn print_help() {
    eprintln!(
        "\
pkrs-runtime — pkrs game player

USAGE:
    pkrs-runtime [OPTIONS]

OPTIONS:
    --project DIR    Run the game project in DIR (loose files, dev saves in DIR/saves)
    --map ID         Start on map ID instead of [start].map (needs --project)
    --warp X,Y       Start on tile X,Y (needs --project)
    --headless       Use NullPlatform (no window, for CI)
    --ticks N        Stop after N simulation ticks (60 Hz); default 600 with --headless
    --no-vsync       Debug: request uncapped frame rate (§3.2)
    -h, --help       Print help
"
    );
}

/// Installs a `tracing` subscriber that writes to stderr (§15.1).
fn init_tracing() -> anyhow::Result<()> {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(DEFAULT_LOG_FILTER));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init()
        .map_err(|e| anyhow::anyhow!(e))
        .context("failed to initialise logging")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_log_filter_parses() {
        assert!(EnvFilter::try_new(DEFAULT_LOG_FILTER).is_ok());
    }

    #[test]
    fn tracing_initialises_once() {
        assert!(init_tracing().is_ok());
        assert!(
            init_tracing().is_err(),
            "a second global subscriber must be rejected"
        );
    }

    #[test]
    fn cli_parse_ticks_and_headless() {
        let opts = RuntimeCliOptions::parse_from(["--headless", "--ticks", "600", "--no-vsync"])
            .expect("parse");
        assert!(opts.headless);
        assert_eq!(opts.ticks, Some(600));
        assert!(!opts.vsync);
    }

    #[test]
    fn cli_headless_without_ticks_is_allowed() {
        let opts = RuntimeCliOptions::parse_from(["--headless"]).expect("parse");
        assert!(opts.headless);
        assert_eq!(opts.ticks, None);
    }

    #[test]
    fn cli_rejects_missing_ticks_value() {
        assert!(RuntimeCliOptions::parse_from(["--ticks"]).is_err());
    }

    #[test]
    fn cli_rejects_negative_ticks() {
        assert!(RuntimeCliOptions::parse_from(["--ticks", "-1"]).is_err());
    }

    #[test]
    fn cli_parses_project_map_and_warp() {
        let opts = RuntimeCliOptions::parse_from([
            "--project",
            "games/demo",
            "--map",
            "house",
            "--warp",
            "3, 4",
        ])
        .expect("parse");
        let project = opts.project_options().expect("project");
        assert_eq!(project.dir, PathBuf::from("games/demo"));
        assert_eq!(project.map.as_deref(), Some("house"));
        assert_eq!(project.warp, Some(GridPos::new(3, 4)));
    }

    #[test]
    fn cli_rejects_bad_warp_and_orphan_map() {
        for bad in [
            &["--project", "d", "--warp", "3"][..],
            &["--project", "d", "--warp", "-1,2"],
            &["--project", "d", "--warp", "a,b"],
            &["--map", "house"],
            &["--warp", "1,1"],
        ] {
            assert!(RuntimeCliOptions::parse_from(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn cli_rejects_unknown_flag() {
        assert!(RuntimeCliOptions::parse_from(["--nope"]).is_err());
    }

    #[test]
    fn headless_exit_accepts_tick_limit_and_quit() {
        let config = RuntimeLoopConfig {
            max_ticks: Some(600),
            ..RuntimeLoopConfig::default()
        };
        assert!(check_headless_exit(LoopExit::TickLimit, &config).is_ok());
        assert!(check_headless_exit(LoopExit::Quit, &config).is_ok());
        assert!(check_headless_exit(LoopExit::FrameLimit, &config).is_err());
    }
}
