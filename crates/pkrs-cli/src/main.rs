//! The `pkrs` command-line tool (§2.3): new, run, check, build, upgrade, plugin.

use anyhow::Context as _;
use tracing_subscriber::EnvFilter;

/// Log filter used when `RUST_LOG` is not set.
const DEFAULT_LOG_FILTER: &str = "info";

fn main() -> anyhow::Result<()> {
    init_tracing()?;
    tracing::debug!("pkrs cli starting");
    // Command parsing (new/run/check/build/upgrade/plugin) lands in a later task (§2.3).
    println!("{}", version_line());
    Ok(())
}

/// Installs a `tracing` subscriber that writes to stderr (§15.1).
///
/// The filter is read from `RUST_LOG`, falling back to [`DEFAULT_LOG_FILTER`].
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

/// The line printed by the CLI until real commands exist.
fn version_line() -> String {
    format!("pkrs {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_tool() {
        assert_eq!(
            version_line(),
            format!("pkrs {}", env!("CARGO_PKG_VERSION"))
        );
    }

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
}
