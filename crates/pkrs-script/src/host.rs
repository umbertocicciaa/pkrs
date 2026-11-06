//! The `ScriptHost` and `ScriptFiles` ports (§4.5, §9.6), in engine vocabulary only.

use core::fmt;
use std::collections::BTreeMap;

use crate::error::{Result, ScriptError};

/// Who a chunk belongs to. Each owner runs with its own `_ENV` table, so plugins cannot clobber
/// each other's (or the game's) globals (§9.6).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ScriptOwner {
    /// The game's own scripts (`scripts/`).
    Game,
    /// A plugin (`plugins/<id>/`).
    Plugin(String),
}

impl ScriptOwner {
    /// Plugin owner `id`.
    #[must_use]
    pub fn plugin(id: impl Into<String>) -> Self {
        Self::Plugin(id.into())
    }

    /// Folders `require` may load from, in search order (§9.6: `scripts/` and the plugin's own
    /// folder).
    #[must_use]
    pub fn require_roots(&self) -> Vec<String> {
        match self {
            Self::Game => vec!["scripts".into()],
            Self::Plugin(id) => vec![format!("plugins/{id}"), "scripts".into()],
        }
    }
}

impl fmt::Display for ScriptOwner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Game => write!(f, "the game"),
            Self::Plugin(id) => write!(f, "plugin `{id}`"),
        }
    }
}

/// A value crossing the Rust/script boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptValue {
    /// `nil`.
    Nil,
    /// Boolean.
    Bool(bool),
    /// Integer.
    Int(i64),
    /// Float.
    Number(f64),
    /// String (UTF-8; invalid bytes are replaced).
    String(String),
    /// Sequence table (`{1, 2, 3}`).
    List(Vec<ScriptValue>),
    /// Table with string keys (integer keys of non-sequence tables are stringified).
    Map(BTreeMap<String, ScriptValue>),
    /// A value that only lives inside the script engine (function, userdata, thread); holds its
    /// type name. Passed back to scripts as `nil`.
    Opaque(&'static str),
}

impl ScriptValue {
    /// `true` for [`ScriptValue::Nil`].
    #[must_use]
    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    /// Integer value (floats with no fraction are accepted).
    #[must_use]
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Int(i) => Some(*i),
            Self::Number(n) if n.fract() == 0.0 && n.is_finite() => Some(*n as i64),
            _ => None,
        }
    }

    /// String value.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }
}

impl From<&str> for ScriptValue {
    fn from(s: &str) -> Self {
        Self::String(s.to_owned())
    }
}

impl From<i64> for ScriptValue {
    fn from(i: i64) -> Self {
        Self::Int(i)
    }
}

impl From<bool> for ScriptValue {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}

/// Safety limits applied to every protected call (§9.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptLimits {
    /// Instructions a single top-level call may execute before it is aborted.
    pub instruction_budget: u64,
    /// Bytes the whole Lua state may allocate (`0` = unlimited).
    pub memory_limit: usize,
}

impl ScriptLimits {
    /// Default instruction budget per call (§9.6: e.g. 10M).
    pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 10_000_000;
    /// Default memory limit (64 MiB).
    pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
}

impl Default for ScriptLimits {
    fn default() -> Self {
        Self {
            instruction_budget: Self::DEFAULT_INSTRUCTION_BUDGET,
            memory_limit: Self::DEFAULT_MEMORY_LIMIT,
        }
    }
}

/// Read-only access to project script files, by project-relative path (`scripts/main.lua`).
///
/// The runtime implements this over the assets VFS; scripts never touch the OS file system.
pub trait ScriptFiles {
    /// Source text of `path`. Missing files are [`ScriptError::Load`].
    fn read_script(&self, path: &str) -> Result<String>;

    /// `true` if `path` exists.
    fn exists(&self, path: &str) -> bool;

    /// Names of the direct subdirectories of `dir` (`"plugins"` → `["day-night", …]`), sorted.
    /// Used for plugin discovery (§9.4). A missing directory, or a source that cannot list,
    /// yields an empty list (the default).
    fn list_dirs(&self, dir: &str) -> Vec<String> {
        let _ = dir;
        Vec::new()
    }
}

/// In-memory [`ScriptFiles`] for tests and generated content.
#[derive(Clone, Debug, Default)]
pub struct MemoryScripts {
    files: BTreeMap<String, String>,
}

impl MemoryScripts {
    /// Empty set.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `path` with `source`.
    #[must_use]
    pub fn with(mut self, path: impl Into<String>, source: impl Into<String>) -> Self {
        self.files.insert(path.into(), source.into());
        self
    }
}

impl ScriptFiles for MemoryScripts {
    fn read_script(&self, path: &str) -> Result<String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| ScriptError::Load {
                path: path.to_owned(),
                reason: "file not found".into(),
            })
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn list_dirs(&self, dir: &str) -> Vec<String> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        let names: std::collections::BTreeSet<&str> = self
            .files
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix)?.split_once('/'))
            .map(|(name, _)| name)
            .collect();
        names.into_iter().map(str::to_owned).collect()
    }
}

/// Handle of a running script task (a coroutine, §9.3).
pub type TaskId = u32;

/// A battle a script asked for with `battle.start_wild` / `battle.start_trainer` (§9.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleRequest {
    /// `battle.start_wild(species, level)`.
    Wild {
        /// Species id.
        species: String,
        /// Level 1–100.
        level: u8,
    },
    /// `battle.start_trainer(id)`.
    Trainer {
        /// Trainer id.
        id: String,
    },
}

/// What a yielded task waits for. The engine resumes it with the answer.
#[derive(Clone, Debug, PartialEq)]
pub enum WaitRequest {
    /// A battle; resume with [`battle_result_value`].
    Battle(BattleRequest),
    /// Any other wait condition (dialogue closed, N ticks, …), as yielded by the script.
    Other(ScriptValue),
}

/// State of a task after it ran.
#[derive(Clone, Debug, PartialEq)]
pub enum TaskStatus {
    /// Suspended until the engine satisfies the request.
    Waiting(WaitRequest),
    /// Returned (its first return value); the task is gone.
    Finished(ScriptValue),
}

/// How a finished battle is handed back to the script that started it:
/// `{ outcome = "win" | "loss" | "fled", fled_side = "player"|"opponent", turns, exp, money }`.
#[must_use]
pub fn battle_result_value(result: &pkrs_battle::BattleResult) -> ScriptValue {
    use pkrs_battle::{BattleOutcome, SideId};
    let mut map = BTreeMap::new();
    let outcome = match result.outcome {
        BattleOutcome::PlayerWin => "win",
        BattleOutcome::OpponentWin => "loss",
        BattleOutcome::Fled { side } => {
            let side = match side {
                SideId::Player => "player",
                SideId::Opponent => "opponent",
            };
            map.insert("fled_side".to_string(), ScriptValue::from(side));
            "fled"
        }
    };
    map.insert("outcome".into(), ScriptValue::from(outcome));
    map.insert("turns".into(), ScriptValue::Int(i64::from(result.turns)));
    map.insert("exp".into(), ScriptValue::Int(i64::from(result.exp)));
    map.insert("money".into(), ScriptValue::Int(i64::from(result.money)));
    ScriptValue::Map(map)
}

/// Engine-owned scripting port (§4.5). `LuaScriptHost` is the v1 adapter.
///
/// Every method is a protected boundary: script errors, runaway loops and memory exhaustion come
/// back as [`ScriptError`]s and are logged with `file:line` and a traceback (§9.6).
pub trait ScriptHost {
    /// Loads and runs the file at `path` in `owner`'s environment; returns the chunk's value.
    fn run_file(&mut self, owner: &ScriptOwner, path: &str) -> Result<ScriptValue>;

    /// Compiles and runs `source` (named `chunk_name` in errors) in `owner`'s environment.
    fn run_source(
        &mut self,
        owner: &ScriptOwner,
        chunk_name: &str,
        source: &str,
    ) -> Result<ScriptValue>;

    /// Calls the global function `name` of `owner` with `args`; returns its first result.
    fn call_function(
        &mut self,
        owner: &ScriptOwner,
        name: &str,
        args: &[ScriptValue],
    ) -> Result<ScriptValue>;

    /// `true` if `owner` defines a global function `name`.
    fn has_function(&mut self, owner: &ScriptOwner, name: &str) -> bool;

    /// Reads `owner`'s global `name` (`Nil` when absent).
    fn global(&mut self, owner: &ScriptOwner, name: &str) -> Result<ScriptValue>;

    /// Bytes currently allocated by the scripting state.
    fn memory_used(&self) -> usize;

    /// Starts `owner`'s global function `name` as a task and runs it until it yields or
    /// returns. A task that errors is aborted and the error returned (§9.6).
    fn start_task(
        &mut self,
        owner: &ScriptOwner,
        name: &str,
        args: &[ScriptValue],
    ) -> Result<(TaskId, TaskStatus)>;

    /// Resumes a waiting task with `answer` (e.g. [`battle_result_value`]).
    fn resume_task(&mut self, task: TaskId, answer: ScriptValue) -> Result<TaskStatus>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn require_roots_are_scripts_and_own_plugin_folder() {
        assert_eq!(ScriptOwner::Game.require_roots(), ["scripts"]);
        assert_eq!(
            ScriptOwner::plugin("resonance").require_roots(),
            ["plugins/resonance", "scripts"]
        );
    }

    #[test]
    fn memory_scripts_report_missing_files() {
        let files = MemoryScripts::new().with("scripts/main.lua", "return 1");
        assert!(files.exists("scripts/main.lua"));
        assert_eq!(files.read_script("scripts/main.lua").unwrap(), "return 1");
        assert!(matches!(
            files.read_script("scripts/nope.lua"),
            Err(ScriptError::Load { .. })
        ));
    }

    #[test]
    fn memory_scripts_list_direct_subdirectories() {
        let files = MemoryScripts::new()
            .with("plugins/b/plugin.toml", "")
            .with("plugins/a/init.lua", "")
            .with("plugins/a/lib/x.lua", "")
            .with("plugins/readme.txt", "");
        assert_eq!(files.list_dirs("plugins"), ["a", "b"]);
        assert!(files.list_dirs("scripts").is_empty());
    }

    #[test]
    fn value_accessors() {
        assert_eq!(ScriptValue::Number(3.0).as_int(), Some(3));
        assert_eq!(ScriptValue::Number(3.5).as_int(), None);
        assert_eq!(ScriptValue::from("x").as_str(), Some("x"));
        assert!(ScriptValue::Nil.is_nil());
    }
}
