//! The `pkrs` command-line tool (§2.3).
//!
//! Implemented: `pkrs new` (one-shot and battle templates), `pkrs run`, `pkrs plugin new` and
//! `pkrs check` (§8.2 validation with source snippets; `--plugins` also prints the resolved
//! load order and hook table, §9.4) and `pkrs pack` (the `game.pak` release archive with
//! texture atlases, §13). `build` and `upgrade` arrive with their milestones (M7).
//!
//! `pkrs check` exit codes: `0` no errors (warnings allowed), `1` errors found (or warnings
//! with `--deny-warnings`), `2` the check could not run (bad arguments, no project).

use std::io::IsTerminal as _;
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Context as _;
use clap::{Parser, Subcommand, ValueEnum};
use pkrs_cli::backend::TexturePackerAdapter;
use pkrs_cli::check;
use pkrs_cli::pack;
use pkrs_cli::plugin::{self, PluginVars};
use pkrs_cli::report::{self, ReportStyle};
use pkrs_cli::run::{self, RUNTIME_ENV, RunRequest};
use pkrs_cli::template::{self, Template, TemplateVars};
use tracing_subscriber::EnvFilter;

/// Log filter used when `RUST_LOG` is not set.
const DEFAULT_LOG_FILTER: &str = "info";

#[derive(Debug, Parser)]
#[command(name = "pkrs", version, about = "Make and run pkrs games")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum TemplateArg {
    /// One small map and the player; no battles.
    OneShot,
    /// Starts straight in a trainer battle; edit `data/` to try creatures and moves.
    Battle,
    /// Two maps, battles and a plugin (not available yet).
    FullGame,
}

impl From<TemplateArg> for Template {
    fn from(t: TemplateArg) -> Self {
        match t {
            TemplateArg::OneShot => Template::OneShot,
            TemplateArg::Battle => Template::Battle,
            TemplateArg::FullGame => Template::FullGame,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a new game project from an embedded template.
    New {
        /// Project name (also the folder name).
        name: String,
        /// Template to start from.
        #[arg(long, value_enum, default_value = "one-shot")]
        template: TemplateArg,
        /// Create the project here instead of `./<name>`.
        #[arg(long)]
        path: Option<PathBuf>,
    },
    /// Run the project from loose files (dev saves go to `<project>/saves/`).
    Run {
        /// Start on this map instead of `[start].map`.
        #[arg(long)]
        map: Option<String>,
        /// Start on this tile (`x,y`).
        #[arg(long, value_parser = parse_warp_arg)]
        warp: Option<(u32, u32)>,
        /// Project folder (default: the nearest folder with a pkrs.toml).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Use a release runtime (prebuilt runtimes arrive in M7; currently informational).
        #[arg(long)]
        release: bool,
        /// Run without a window (CI).
        #[arg(long)]
        headless: bool,
        /// Stop after N simulation ticks.
        #[arg(long)]
        ticks: Option<u64>,
    },
    /// Validate the project: manifest, plugins, data, maps and Lua syntax. Exit code 1 on
    /// errors (2 when the check cannot run).
    Check {
        /// Also load the enabled plugins and print the resolved hook table.
        #[arg(long)]
        plugins: bool,
        /// Fail (exit code 1) on warnings too.
        #[arg(long)]
        deny_warnings: bool,
        /// Project folder (default: the nearest folder with a pkrs.toml).
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Pack the project into a `game.pak` release archive (texture atlases included).
    Pack {
        /// Where to write the archive (default: `<project>/dist/game.pak`).
        #[arg(long)]
        out: Option<PathBuf>,
        /// Project folder (default: the nearest folder with a pkrs.toml).
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Work with plugins (§9.4).
    Plugin {
        #[command(subcommand)]
        command: PluginCommand,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Scaffold `plugins/<id>/` with a manifest and hook examples.
    New {
        /// Plugin name; its id (and folder name) is the lowercase, `-`-joined form.
        name: String,
        /// Project folder (default: the nearest folder with a pkrs.toml).
        #[arg(long)]
        project: Option<PathBuf>,
    },
}

fn parse_warp_arg(raw: &str) -> Result<(u32, u32), String> {
    run::parse_warp(raw).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    if let Err(err) = init_tracing() {
        eprintln!("{err:#}");
    }
    match execute(Cli::parse()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(EXIT_CANNOT_RUN)
        }
    }
}

/// Exit code of `pkrs check` when content has errors.
const EXIT_INVALID: u8 = 1;
/// Exit code when a command could not run at all.
const EXIT_CANNOT_RUN: u8 = 2;

fn execute(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::New {
            name,
            template,
            path,
        } => {
            let vars = TemplateVars::for_project(&name)?;
            let dest = path.unwrap_or_else(|| PathBuf::from(&name));
            let files = template::scaffold(template.into(), &dest, &vars)?;
            println!(
                "Created {} ({} files) in {}",
                vars,
                files.len(),
                dest.display()
            );
            println!("Next: cd {} && pkrs run", dest.display());
            Ok(ExitCode::SUCCESS)
        }
        Command::Check {
            plugins,
            deny_warnings,
            project,
        } => {
            let project = project_root(project)?;
            let report = check::check_project(&project, plugins)?;
            print!("{report}");
            if !report.diagnostics.is_empty() {
                let style = ReportStyle {
                    color: std::env::var_os("NO_COLOR").is_none()
                        && std::io::stderr().is_terminal(),
                    ..ReportStyle::default()
                };
                let source = |path: &str| check::read_source(&project, path);
                eprint!(
                    "{}",
                    report::render_diagnostics(&report.diagnostics, &source, style)
                );
            }
            Ok(if report.passes(deny_warnings) {
                println!("check passed");
                ExitCode::SUCCESS
            } else {
                let why = if report.is_ok() {
                    "warnings are denied"
                } else {
                    "errors found"
                };
                eprintln!("check failed: {why}");
                ExitCode::from(EXIT_INVALID)
            })
        }
        Command::Plugin {
            command: PluginCommand::New { name, project },
        } => {
            let vars = PluginVars::for_plugin(&name)?;
            let project = project_root(project)?;
            let out = plugin::new_plugin(&project, &vars)?;
            println!(
                "Created {} ({} files) in {}",
                vars,
                out.files.len(),
                out.dir.display()
            );
            match &out.enable_hint {
                Some(hint) => println!("Next: enable it in pkrs.toml:\n{hint}"),
                None => println!("Next: pkrs check --plugins"),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Pack { out, project } => {
            let project = project_root(project)?;
            let out = out.unwrap_or_else(|| pack::default_out(&project));
            let report = pack::pack_project(&project, &out, &TexturePackerAdapter)
                .with_context(|| format!("cannot pack {}", project.display()))?;
            println!("{report}");
            Ok(ExitCode::SUCCESS)
        }
        Command::Run {
            map,
            warp,
            project,
            release,
            headless,
            ticks,
        } => {
            let project = project_root(project)?;
            let exe = std::env::current_exe().context("cannot locate the pkrs executable")?;
            let runtime =
                run::locate_runtime(std::env::var_os(RUNTIME_ENV).map(PathBuf::from), &exe)?;
            if release {
                tracing::info!(
                    "--release: using the locally built runtime until prebuilt runtimes ship (M7)"
                );
            }
            let args = run::runtime_args(&RunRequest {
                project,
                map,
                warp,
                headless,
                ticks,
            });
            tracing::debug!(runtime = %runtime.display(), ?args, "launching runtime");
            let status = std::process::Command::new(&runtime)
                .args(&args)
                .status()
                .with_context(|| format!("cannot start {}", runtime.display()))?;
            Ok(match status.code() {
                Some(0) => ExitCode::SUCCESS,
                Some(code) => ExitCode::from(u8::try_from(code).unwrap_or(1)),
                None => ExitCode::FAILURE,
            })
        }
    }
}

/// `--project` if given, else the nearest folder at or above the current one with a pkrs.toml.
fn project_root(project: Option<PathBuf>) -> anyhow::Result<PathBuf> {
    let start = match project {
        Some(p) => p,
        None => std::env::current_dir().context("cannot read the current directory")?,
    };
    Ok(run::find_project_root(&start)?)
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
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_new_with_defaults() {
        let cli = Cli::try_parse_from(["pkrs", "new", "demo"]).unwrap();
        let Command::New {
            name,
            template,
            path,
        } = cli.command
        else {
            panic!("expected new")
        };
        assert_eq!(name, "demo");
        assert_eq!(template, TemplateArg::OneShot);
        assert!(path.is_none());
        let cli = Cli::try_parse_from(["pkrs", "new", "x", "--template", "battle"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::New {
                template: TemplateArg::Battle,
                ..
            }
        ));
        let cli = Cli::try_parse_from(["pkrs", "new", "x", "--template", "full-game"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::New {
                template: TemplateArg::FullGame,
                ..
            }
        ));
    }

    #[test]
    fn parses_run_flags() {
        let cli = Cli::try_parse_from([
            "pkrs",
            "run",
            "--map",
            "house",
            "--warp",
            "5,7",
            "--release",
            "--headless",
            "--ticks",
            "60",
        ])
        .unwrap();
        let Command::Run {
            map,
            warp,
            release,
            headless,
            ticks,
            project,
        } = cli.command
        else {
            panic!("expected run")
        };
        assert_eq!(map.as_deref(), Some("house"));
        assert_eq!(warp, Some((5, 7)));
        assert!(release && headless);
        assert_eq!(ticks, Some(60));
        assert!(project.is_none());
        assert!(Cli::try_parse_from(["pkrs", "run", "--warp", "5"]).is_err());
        assert!(
            Cli::try_parse_from(["pkrs"]).is_err(),
            "a subcommand is required"
        );
    }

    #[test]
    fn parses_check_flags() {
        let cli = Cli::try_parse_from(["pkrs", "check", "--plugins"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Check {
                plugins: true,
                deny_warnings: false,
                project: None
            }
        ));
        let cli = Cli::try_parse_from(["pkrs", "check", "--project", "game"]).unwrap();
        let Command::Check {
            plugins,
            deny_warnings,
            project,
        } = cli.command
        else {
            panic!("expected check")
        };
        assert!(!plugins && !deny_warnings);
        assert_eq!(project, Some(PathBuf::from("game")));
        let cli = Cli::try_parse_from(["pkrs", "check", "--deny-warnings"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Check {
                deny_warnings: true,
                ..
            }
        ));
    }

    #[test]
    fn parses_plugin_new() {
        let cli = Cli::try_parse_from(["pkrs", "plugin", "new", "Day Night"]).unwrap();
        let Command::Plugin {
            command: PluginCommand::New { name, project },
        } = cli.command
        else {
            panic!("expected plugin new")
        };
        assert_eq!(name, "Day Night");
        assert!(project.is_none());
        let cli = Cli::try_parse_from(["pkrs", "plugin", "new", "x", "--project", "game"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Plugin {
                command: PluginCommand::New {
                    project: Some(_),
                    ..
                }
            }
        ));
        assert!(Cli::try_parse_from(["pkrs", "plugin"]).is_err());
        assert!(Cli::try_parse_from(["pkrs", "plugin", "new"]).is_err());
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
