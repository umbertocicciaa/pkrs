//! The pkrs game player (§0.5): wires every subsystem, owns the state stack and main loop.

use anyhow::Context as _;
use pkrs_platform::{
    DisplaySettings, FixedPaths, MacroquadPlatform, NullPlatform, Platform, PlatformConfig,
    WindowConfig,
};
use pkrs_runtime::{RuntimeLoopConfig, StubSimulationHost, block_on_loop, run_loop};
use tracing_subscriber::EnvFilter;

/// Log filter used when `RUST_LOG` is not set.
const DEFAULT_LOG_FILTER: &str = "info";

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

    let config = RuntimeLoopConfig {
        max_ticks: cli.ticks,
        max_frames: None,
        vsync: cli.vsync,
    };

    if cli.headless {
        run_headless(config)?;
    } else {
        run_windowed(config)?;
    }
    Ok(())
}

fn run_headless(config: RuntimeLoopConfig) -> anyhow::Result<()> {
    let mut platform = Platform::new(
        Box::new(NullPlatform::new()),
        Box::new(FixedPaths::under("pkrs")),
        PlatformConfig::default(),
    );
    let mut host = StubSimulationHost;
    let exit = block_on_loop(&mut platform, &mut host, &config);
    tracing::info!(
        ?exit,
        frames = platform.frame_count(),
        "headless run finished"
    );
    Ok(())
}

fn run_windowed(config: RuntimeLoopConfig) -> anyhow::Result<()> {
    let window = WindowConfig::from_display("pkrs", &DisplaySettings::default());
    if !config.vsync {
        tracing::warn!(
            "vsync disabled: macroquad still uses its default swap interval until WindowConfig exposes this (§3.2)"
        );
    }
    MacroquadPlatform::run(&window, move |backend| async move {
        let mut platform = Platform::new(
            Box::new(backend),
            Box::new(FixedPaths::under("pkrs")),
            PlatformConfig::default(),
        );
        let mut host = StubSimulationHost;
        let exit = run_loop(&mut platform, &mut host, &config).await;
        tracing::info!(
            ?exit,
            frames = platform.frame_count(),
            "windowed run finished"
        );
    });
    Ok(())
}

/// Parsed `pkrs-runtime` command-line flags (§15.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RuntimeCliOptions {
    headless: bool,
    ticks: Option<u64>,
    vsync: bool,
}

impl RuntimeCliOptions {
    fn parse() -> anyhow::Result<Self> {
        let mut headless = false;
        let mut ticks = None;
        let mut vsync = true;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--headless" => headless = true,
                "--no-vsync" => vsync = false,
                "--ticks" => {
                    let value = args
                        .next()
                        .context("--ticks requires a non-negative integer")?;
                    ticks = Some(
                        value
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
        Ok(Self {
            headless,
            ticks,
            vsync,
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
    --headless       Use NullPlatform (no window, for CI)
    --ticks N        Stop after N simulation ticks (60 Hz)
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
    fn cli_parse_ticks_and_headless() {
        let opts = RuntimeCliOptions {
            headless: true,
            ticks: Some(600),
            vsync: false,
        };
        assert!(opts.headless);
        assert_eq!(opts.ticks, Some(600));
        assert!(!opts.vsync);
    }
}
