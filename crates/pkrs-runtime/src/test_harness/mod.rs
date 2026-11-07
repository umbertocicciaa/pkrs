//! Headless scripted-input test harness (§15.2, §14.4): runs a project with the Null adapters
//! and drives it from a `tests/*.lua` scenario.
//!
//! ```text
//! pkrs-runtime --headless --project games/demo --script tests/talk_to_npc.lua
//! ```
//!
//! The scenario runs in its own sandboxed Lua state ([`LuaScenario`]), separate from the game's
//! scripts and plugins. Each time it needs time to pass it hands back a tick count and the
//! actions to hold; the [`ScenarioDriver`] turns those actions into key presses on a fresh
//! [`NullPlatform`] frame per tick and runs the project's [`SimulationHost`] through
//! `poll_input` → `begin_tick` → `simulation_tick` → `render`, exactly like the main loop at
//! one tick per frame (§3.1, §3.3). Nothing depends on wall-clock time, and the game scripts'
//! RNG is seeded with [`ScenarioOptions::seed`], so a scenario replays identically.
//!
//! A failed expectation, a script error or running past [`ScenarioOptions::max_ticks`] fails
//! the scenario with a [`ScenarioFailure`] (`file:line`, check, expected vs actual, tick); the
//! CLI prints it and exits non-zero.

mod snapshot;

pub use snapshot::{
    STATE_BATTLE, STATE_CHOICE, STATE_DIALOGUE, STATE_OVERWORLD, STATE_WARP, snapshot, state_name,
};

use std::fmt;
use std::path::{Path, PathBuf};

use pkrs_core::Pcg64Rng;
use pkrs_platform::{
    Action, FixedPaths, Key, NullFrame, NullPlatform, Platform, PlatformConfig, block_on,
};
use pkrs_render::DrawLog;
use pkrs_script::scenario::{ScenarioFailure, ScenarioStep};
use pkrs_script::{LuaScenario, ScriptLimits};

use crate::fixed_step::TICK_DT;
use crate::main_loop::SimulationHost;
use crate::overworld::UiRequest;
use crate::plugins::{PluginStartError, start_project_scripts};
use crate::project::{Project, ProjectError, ProjectHost};
use crate::wiring::headless_renderer;

/// Folder of a project that holds its scenarios.
pub const SCENARIO_DIR: &str = "tests";

/// Default tick budget of one scenario (10 minutes of game time at 60 Hz).
pub const DEFAULT_SCENARIO_TICKS: u64 = 36_000;

/// Default seed of the game scripts' RNG in a scenario run.
pub const DEFAULT_SCENARIO_SEED: u64 = 0x706b_7273;

/// How a scenario is run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScenarioOptions {
    /// The scenario fails if it needs more ticks than this.
    pub max_ticks: u64,
    /// Seed of the game scripts' `math.random`.
    pub seed: u64,
    /// Sandbox limits of the scenario script itself.
    pub limits: ScriptLimits,
}

impl Default for ScenarioOptions {
    fn default() -> Self {
        Self {
            max_ticks: DEFAULT_SCENARIO_TICKS,
            seed: DEFAULT_SCENARIO_SEED,
            limits: ScriptLimits::default(),
        }
    }
}

/// Why a scenario could not be run at all (as opposed to running and failing).
#[derive(Debug, thiserror::Error)]
pub enum HarnessError {
    /// The scenario file could not be read.
    #[error("cannot read scenario `{path}`: {source}")]
    Read {
        /// Path of the scenario.
        path: PathBuf,
        /// I/O error.
        source: std::io::Error,
    },
    /// The project's `tests/` folder could not be listed.
    #[error("cannot list scenarios in `{path}`: {source}")]
    Discover {
        /// The folder.
        path: PathBuf,
        /// I/O error.
        source: std::io::Error,
    },
    /// The project could not be opened or started.
    #[error(transparent)]
    Project(#[from] ProjectError),
    /// The project's plugins could not be loaded.
    #[error(transparent)]
    Plugins(#[from] PluginStartError),
}

/// How a scenario ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioOutcome {
    /// The script returned and every expectation held.
    Passed,
    /// An expectation failed, the script raised an error or ran out of ticks.
    Failed(ScenarioFailure),
}

/// Result of one scenario run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioReport {
    /// Scenario name (its path, e.g. `tests/smoke.lua`).
    pub name: String,
    /// Simulation ticks the game ran.
    pub ticks: u64,
    /// Pass or failure.
    pub outcome: ScenarioOutcome,
}

impl ScenarioReport {
    /// `true` when the scenario passed.
    #[must_use]
    pub fn passed(&self) -> bool {
        matches!(self.outcome, ScenarioOutcome::Passed)
    }

    /// The failure, if any.
    #[must_use]
    pub fn failure(&self) -> Option<&ScenarioFailure> {
        match &self.outcome {
            ScenarioOutcome::Passed => None,
            ScenarioOutcome::Failed(f) => Some(f),
        }
    }
}

/// `PASS tests/smoke.lua (120 ticks)`, or `FAIL …` followed by the indented failure.
impl fmt::Display for ScenarioReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.outcome {
            ScenarioOutcome::Passed => write!(f, "PASS {} ({} ticks)", self.name, self.ticks),
            ScenarioOutcome::Failed(failure) => {
                write!(f, "FAIL {} ({} ticks)", self.name, self.ticks)?;
                for line in failure.to_string().lines() {
                    write!(f, "\n  {line}")?;
                }
                Ok(())
            }
        }
    }
}

/// Runs a [`ProjectHost`] one tick at a time with scripted input.
pub struct ScenarioDriver {
    host: ProjectHost,
    log: DrawLog,
    dialogues_opened: u64,
    last_overlay: Option<UiRequest>,
}

impl ScenarioDriver {
    /// Starts `project` headless (Null renderer, game scripts seeded with `seed`).
    pub fn start(project: &Project, seed: u64) -> Result<Self, HarnessError> {
        let (scripts, _plugins) =
            start_project_scripts(project, Box::new(Pcg64Rng::from_seed(seed)))?;
        let (renderer, log) = headless_renderer(project.display());
        let mut host = ProjectHost::with_scripts(project, renderer, scripts)?;
        host.set_battle_seed(seed);
        host.set_encounter_seed(seed);
        let last_overlay = host.overlay().map(|o| o.request().clone());
        Ok(Self {
            host,
            log,
            dialogues_opened: u64::from(last_overlay.is_some()),
            last_overlay,
        })
    }

    /// The game being driven.
    #[must_use]
    pub fn host(&self) -> &ProjectHost {
        &self.host
    }

    /// What a scenario sees right now.
    #[must_use]
    pub fn snapshot(&self) -> pkrs_script::scenario::ScenarioSnapshot {
        // Battle messages count as text boxes too.
        let opened = self
            .dialogues_opened
            .saturating_add(self.host.battle_texts_shown());
        snapshot(&self.host, opened)
    }

    /// The keys that press `actions` (the first key bound to each). Errors name an action
    /// that is unknown or has no key.
    pub fn keys_for(&self, actions: &[String]) -> Result<Vec<Key>, String> {
        let input = self.host.input();
        actions
            .iter()
            .map(|name| {
                let action: Action = input
                    .action(name)
                    .ok_or_else(|| format!("unknown action `{name}`"))?;
                input
                    .bound_keys(action)
                    .first()
                    .copied()
                    .ok_or_else(|| format!("action `{name}` has no key bound"))
            })
            .collect()
    }

    /// Runs one simulation tick with `keys` held during its frame, then renders it.
    pub fn tick(&mut self, keys: &[Key]) {
        let frame = NullFrame::with_keys(keys.iter().copied());
        let mut platform = Platform::new(
            Box::new(NullPlatform::new().with_frames([frame])),
            Box::new(FixedPaths::under("pkrs-scenario")),
            PlatformConfig::default(),
        );
        block_on(platform.next_frame());
        self.host.poll_input(&platform);
        self.host.begin_tick();
        self.host.simulation_tick(TICK_DT);
        self.host.render(&platform, 1.0);
        self.log.clear();

        let overlay = self.host.overlay().map(|o| o.request().clone());
        if overlay.is_some() && overlay != self.last_overlay {
            self.dialogues_opened = self.dialogues_opened.saturating_add(1);
        }
        self.last_overlay = overlay;
    }

    /// Runs `scenario` to the end, or until the game has run `max_ticks` ticks.
    pub fn run(&mut self, scenario: &mut LuaScenario, max_ticks: u64) -> ScenarioReport {
        let outcome = loop {
            match scenario.resume(self.snapshot()) {
                ScenarioStep::Finished => break ScenarioOutcome::Passed,
                ScenarioStep::Failed(failure) => break ScenarioOutcome::Failed(failure),
                ScenarioStep::Advance { ticks, held } => {
                    let keys = match self.keys_for(&held) {
                        Ok(keys) => keys,
                        Err(message) => {
                            break ScenarioOutcome::Failed(
                                ScenarioFailure::new("input", self.host.ticks()).message(message),
                            );
                        }
                    };
                    if let Some(failure) = self.advance(ticks, &keys, max_ticks) {
                        break ScenarioOutcome::Failed(failure);
                    }
                }
            }
        };
        ScenarioReport {
            name: scenario.name().to_owned(),
            ticks: self.host.ticks(),
            outcome,
        }
    }

    fn advance(&mut self, ticks: u32, keys: &[Key], max_ticks: u64) -> Option<ScenarioFailure> {
        for _ in 0..ticks {
            if self.host.ticks() >= max_ticks {
                return Some(
                    ScenarioFailure::new("tick limit", self.host.ticks())
                        .message(format!("the scenario needs more than {max_ticks} ticks")),
                );
            }
            self.tick(keys);
        }
        None
    }
}

/// Runs the scenario `source` (named `name` in failures) against a fresh start of `project`.
pub fn run_scenario_source(
    project: &Project,
    name: &str,
    source: &str,
    options: &ScenarioOptions,
) -> Result<ScenarioReport, HarnessError> {
    let mut driver = ScenarioDriver::start(project, options.seed)?;
    let mut scenario = match LuaScenario::load(name, source, options.limits) {
        Ok(scenario) => scenario,
        Err(err) => {
            return Ok(ScenarioReport {
                name: name.to_owned(),
                ticks: 0,
                outcome: ScenarioOutcome::Failed(
                    ScenarioFailure::new("load", 0).message(err.to_string()),
                ),
            });
        }
    };
    Ok(driver.run(&mut scenario, options.max_ticks))
}

/// Reads the scenario at `path` and runs it (see [`run_scenario_source`]).
pub fn run_scenario(
    project: &Project,
    path: &Path,
    name: &str,
    options: &ScenarioOptions,
) -> Result<ScenarioReport, HarnessError> {
    let source = std::fs::read_to_string(path).map_err(|source| HarnessError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    run_scenario_source(project, name, &source, options)
}

/// A scenario file and the name it reports under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioFile {
    /// Where the file is.
    pub path: PathBuf,
    /// Name in reports and failure locations (`tests/smoke.lua`).
    pub name: String,
}

/// Resolves the `--script` argument: a relative path is looked up in the project folder
/// first, then in the working directory.
#[must_use]
pub fn resolve_script(project_dir: &Path, script: &Path) -> ScenarioFile {
    let name = script.to_string_lossy().replace('\\', "/");
    let in_project = project_dir.join(script);
    let path = if script.is_relative() && in_project.is_file() {
        in_project
    } else {
        script.to_path_buf()
    };
    ScenarioFile { path, name }
}

/// Every `tests/*.lua` scenario of the project in `project_dir`, sorted by name. A project
/// without a `tests/` folder has none.
pub fn discover_scenarios(project_dir: &Path) -> Result<Vec<ScenarioFile>, HarnessError> {
    let dir = project_dir.join(SCENARIO_DIR);
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(&dir).map_err(|source| HarnessError::Discover {
        path: dir.clone(),
        source,
    })?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| HarnessError::Discover {
            path: dir.clone(),
            source,
        })?;
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "lua") {
            let file = entry.file_name().to_string_lossy().into_owned();
            found.push(ScenarioFile {
                name: format!("{SCENARIO_DIR}/{file}"),
                path,
            });
        }
    }
    found.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(found)
}

/// Runs every scenario of `project` (each against a fresh start), in name order.
pub fn run_project_scenarios(
    project: &Project,
    project_dir: &Path,
    options: &ScenarioOptions,
) -> Result<Vec<ScenarioReport>, HarnessError> {
    discover_scenarios(project_dir)?
        .iter()
        .map(|file| run_scenario(project, &file.path, &file.name, options))
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_script::scenario::SourceLocation;

    #[test]
    fn report_display_indents_the_failure() {
        let pass = ScenarioReport {
            name: "tests/smoke.lua".into(),
            ticks: 12,
            outcome: ScenarioOutcome::Passed,
        };
        assert_eq!(pass.to_string(), "PASS tests/smoke.lua (12 ticks)");
        assert!(pass.passed() && pass.failure().is_none());
        let fail = ScenarioReport {
            name: "tests/talk.lua".into(),
            ticks: 40,
            outcome: ScenarioOutcome::Failed(
                ScenarioFailure::new("expect_state", 40)
                    .at(Some(SourceLocation {
                        file: "tests/talk.lua".into(),
                        line: 7,
                    }))
                    .values("Dialogue", "Overworld"),
            ),
        };
        assert_eq!(
            fail.to_string(),
            "FAIL tests/talk.lua (40 ticks)\n  tests/talk.lua:7: expect_state failed at tick 40\n    expected: Dialogue\n      actual: Overworld"
        );
        assert!(!fail.passed());
    }

    #[test]
    fn resolve_script_prefers_the_project_folder() {
        let dir = std::env::temp_dir().join(format!("pkrs-resolve-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("tests/a.lua"), "").unwrap();
        let found = resolve_script(&dir, Path::new("tests/a.lua"));
        assert_eq!(found.path, dir.join("tests/a.lua"));
        assert_eq!(found.name, "tests/a.lua");
        let missing = resolve_script(&dir, Path::new("other/b.lua"));
        assert_eq!(missing.path, PathBuf::from("other/b.lua"));

        std::fs::write(dir.join("tests/b.lua"), "").unwrap();
        std::fs::write(dir.join("tests/notes.txt"), "").unwrap();
        let names: Vec<String> = discover_scenarios(&dir)
            .unwrap()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["tests/a.lua", "tests/b.lua"]);
        assert!(discover_scenarios(&dir.join("nope")).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn default_options_are_bounded_and_seeded() {
        let options = ScenarioOptions::default();
        assert_eq!(options.max_ticks, DEFAULT_SCENARIO_TICKS);
        assert_eq!(options.seed, DEFAULT_SCENARIO_SEED);
    }
}
