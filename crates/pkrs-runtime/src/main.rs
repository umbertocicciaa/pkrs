//! The pkrs game player (§0.5): wires every subsystem, owns the state stack and main loop.

use anyhow::Context as _;
use tracing_subscriber::EnvFilter;

/// Log filter used when `RUST_LOG` is not set.
const DEFAULT_LOG_FILTER: &str = "info";

fn main() -> anyhow::Result<()> {
    init_tracing()?;
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "pkrs-runtime starting");
    // Subsystem wiring and the main loop land in later milestones (§3, §4).
    tracing::info!("pkrs-runtime has nothing to run yet; exiting");
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
}
