//! Scripted-input test scenarios (`tests/*.lua`, §15.2): engine-side types.
//!
//! A scenario is a Lua script that drives the runtime like a player would (`press("confirm")`,
//! `wait_ticks(10)`) and checks what happened (`expect_state("Dialogue")`,
//! `expect_flag("met_guide")`). It runs in its own sandboxed Lua state
//! ([`LuaScenario`](crate::LuaScenario)), never in the game's: it sees only the test API and the
//! safe standard library, and it reads the game through a [`ScenarioSnapshot`] the runtime takes
//! between ticks.
//!
//! The runtime drives it in a loop:
//!
//! 1. take a [`ScenarioSnapshot`] of the game,
//! 2. [`resume`](crate::LuaScenario::resume) the scenario with it; the script runs until it
//!    needs time to pass and returns a [`ScenarioStep`],
//! 3. on [`ScenarioStep::Advance`], run that many ticks with exactly those actions held, then
//!    go back to 1; [`ScenarioStep::Finished`] passes, [`ScenarioStep::Failed`] fails.
//!
//! Everything here is plain data, so the runtime never sees a Lua type.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use pkrs_core::{Direction, GridPos};

use crate::host::ScriptValue;

/// Ticks [`wait_until`-style](crate::LuaScenario) helpers wait by default before failing.
pub const DEFAULT_WAIT_TICKS: u32 = 600;

/// Where in a scenario file something happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceLocation {
    /// Script path as the scenario was loaded (`tests/talk_to_npc.lua`).
    pub file: String,
    /// 1-based line.
    pub line: u32,
}

impl fmt::Display for SourceLocation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.file, self.line)
    }
}

/// A player-visible position: tile and facing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Placement {
    /// Tile.
    pub pos: GridPos,
    /// Facing.
    pub facing: Direction,
}

/// What a scenario can observe about the game between two ticks.
///
/// The runtime fills it in; the test API's queries (`state()`, `flag(name)`, …) and
/// expectations read it. Missing information stays at its default (empty / `None`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScenarioSnapshot {
    /// Simulation ticks run so far.
    pub tick: u64,
    /// Name of the active game state: `Overworld`, `Dialogue`, `Choice`, `Warp`, `Battle`,
    /// `Evolution`, …
    pub state: String,
    /// Active map id.
    pub map: Option<String>,
    /// Active text language (§5.5), e.g. `en`.
    pub language: Option<String>,
    /// Player tile and facing.
    pub player: Option<Placement>,
    /// `true` when the player is not mid-step.
    pub player_idle: bool,
    /// `true` while a script task or `player.lock()` keeps the player from moving.
    pub player_locked: bool,
    /// Text of the open text box, or the prompt of the open choice list.
    pub dialogue: Option<String>,
    /// Text boxes and choice lists opened so far (tells two identical boxes in a row apart).
    pub dialogues_opened: u64,
    /// Options of the open choice list (empty when none is open).
    pub choices: Vec<String>,
    /// Highlighted option of the open choice list (0-based).
    pub cursor: Option<usize>,
    /// Global flags.
    pub flags: BTreeMap<String, bool>,
    /// Global variables.
    pub vars: BTreeMap<String, ScriptValue>,
    /// Items in the bag.
    pub items: BTreeMap<String, u32>,
    /// NPCs of the active map, by script id.
    pub npcs: BTreeMap<String, Placement>,
    /// Action names `press` / `hold` accept (`confirm`, `up`, `resonance:toggle`, …).
    pub actions: BTreeSet<String>,
    /// The battle on screen while the state is `Battle` (§7.2).
    pub battle: Option<BattleSnapshot>,
    /// The player's party (empty before the first battle loaded it).
    pub party: Vec<PartyMemberSnapshot>,
}

/// One member of the player's party (`party()` in the test API).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PartyMemberSnapshot {
    /// Species id.
    pub species: String,
    /// Display name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// Total experience.
    pub exp: u32,
    /// Known move ids, in slot order.
    pub moves: Vec<String>,
    /// Maximum HP at the current level.
    pub max_hp: u32,
    /// Attack at the current level.
    pub attack: u32,
    /// Defense at the current level.
    pub defense: u32,
    /// Speed at the current level.
    pub speed: u32,
}

/// What a scenario sees of a running battle (`battle()` in the test API).
///
/// While a battle message is shown, [`ScenarioSnapshot::dialogue`] holds its text; while a
/// battle menu is open, [`ScenarioSnapshot::choices`] and [`ScenarioSnapshot::cursor`] hold
/// its entries and highlighted row, so `choose(i)` works in menus too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattleSnapshot {
    /// Open menu: `action`, `fight`, `bag` or `party` (`None` while events play back).
    pub menu: Option<String>,
    /// Current turn (1-based).
    pub turn: u32,
    /// The player's active creature, as displayed (HP follows the HP bar animation).
    pub player: Option<BattlerSnapshot>,
    /// The opponent's active creature, as displayed.
    pub opponent: Option<BattlerSnapshot>,
}

/// One active creature in a [`BattleSnapshot`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BattlerSnapshot {
    /// Display name.
    pub name: String,
    /// Level.
    pub level: u8,
    /// HP shown on the bar.
    pub hp: u32,
    /// Maximum HP.
    pub max_hp: u32,
}

/// What the scenario asks the runtime to do next.
#[derive(Clone, Debug, PartialEq)]
pub enum ScenarioStep {
    /// Run `ticks` simulation ticks with exactly `held` actions held, then resume.
    Advance {
        /// Number of ticks (at least 1).
        ticks: u32,
        /// Action names held during those ticks, sorted.
        held: Vec<String>,
    },
    /// The script returned: every expectation held.
    Finished,
    /// An expectation failed or the script raised an error.
    Failed(ScenarioFailure),
}

/// Why a scenario failed, with enough context to find and fix it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioFailure {
    /// The check that failed (`expect_state`, `wait_until`, `error`, …).
    pub check: String,
    /// Where in the scenario it happened, when known.
    pub location: Option<SourceLocation>,
    /// Tick the game was at.
    pub tick: u64,
    /// What the scenario expected.
    pub expected: Option<String>,
    /// What the game had.
    pub actual: Option<String>,
    /// Extra explanation (the custom message of `expect(cond, msg)`, a script error, …).
    pub message: Option<String>,
}

impl ScenarioFailure {
    /// A failure of `check` at `tick` with no details yet.
    #[must_use]
    pub fn new(check: impl Into<String>, tick: u64) -> Self {
        Self {
            check: check.into(),
            location: None,
            tick,
            expected: None,
            actual: None,
            message: None,
        }
    }

    /// Sets the location.
    #[must_use]
    pub fn at(mut self, location: Option<SourceLocation>) -> Self {
        self.location = location;
        self
    }

    /// Sets the expected and actual values.
    #[must_use]
    pub fn values(mut self, expected: impl Into<String>, actual: impl Into<String>) -> Self {
        self.expected = Some(expected.into());
        self.actual = Some(actual.into());
        self
    }

    /// Sets the message.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// ```text
/// tests/talk.lua:12: expect_state failed at tick 42
///   expected: Dialogue
///     actual: Overworld
/// ```
impl fmt::Display for ScenarioFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(location) = &self.location {
            write!(f, "{location}: ")?;
        }
        write!(f, "{} failed at tick {}", self.check, self.tick)?;
        if let Some(message) = &self.message {
            write!(f, ": {message}")?;
        }
        if let Some(expected) = &self.expected {
            write!(f, "\n  expected: {expected}")?;
        }
        if let Some(actual) = &self.actual {
            write!(f, "\n    actual: {actual}")?;
        }
        Ok(())
    }
}

impl std::error::Error for ScenarioFailure {}

/// Formats `value` the way a scenario author wrote it: `nil`, `true`, `3`, `"text"`,
/// `{1, 2}`, `{a = 1}`.
#[must_use]
pub fn format_value(value: &ScriptValue) -> String {
    match value {
        ScriptValue::Nil => "nil".into(),
        ScriptValue::Bool(b) => b.to_string(),
        ScriptValue::Int(i) => i.to_string(),
        ScriptValue::Number(n) => {
            if n.fract() == 0.0 && n.is_finite() {
                format!("{n:.1}")
            } else {
                n.to_string()
            }
        }
        ScriptValue::String(s) => format!("{s:?}"),
        ScriptValue::List(items) => {
            let parts: Vec<String> = items.iter().map(format_value).collect();
            format!("{{{}}}", parts.join(", "))
        }
        ScriptValue::Map(map) => {
            let parts: Vec<String> = map
                .iter()
                .map(|(k, v)| format!("{k} = {}", format_value(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
        ScriptValue::Opaque(kind) => format!("<{kind}>"),
        ScriptValue::Npc(id) => format!("npc({id:?})"),
    }
}

/// Lua-style equality for expectations: integers and floats compare by value (`3 == 3.0`),
/// tables compare element by element.
#[must_use]
pub fn values_equal(a: &ScriptValue, b: &ScriptValue) -> bool {
    match (a, b) {
        (ScriptValue::Int(x), ScriptValue::Number(y))
        | (ScriptValue::Number(y), ScriptValue::Int(x)) => (*x as f64) == *y,
        (ScriptValue::List(x), ScriptValue::List(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(a, b)| values_equal(a, b))
        }
        (ScriptValue::Map(x), ScriptValue::Map(y)) => {
            x.len() == y.len()
                && x.iter()
                    .zip(y)
                    .all(|((ka, va), (kb, vb))| ka == kb && values_equal(va, vb))
        }
        _ => a == b,
    }
}

/// `(x, y)` or `(x, y) facing up`.
#[must_use]
pub fn format_placement(pos: GridPos, facing: Option<Direction>) -> String {
    match facing {
        Some(facing) => format!("{pos} facing {facing}"),
        None => pos.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_display_has_location_tick_and_values() {
        let failure = ScenarioFailure::new("expect_state", 42)
            .at(Some(SourceLocation {
                file: "tests/talk.lua".into(),
                line: 12,
            }))
            .values("Dialogue", "Overworld");
        assert_eq!(
            failure.to_string(),
            "tests/talk.lua:12: expect_state failed at tick 42\n  expected: Dialogue\n    actual: Overworld"
        );
    }

    #[test]
    fn failure_display_without_location_or_values() {
        let failure = ScenarioFailure::new("expect", 3).message("door is open");
        assert_eq!(failure.to_string(), "expect failed at tick 3: door is open");
    }

    #[test]
    fn values_format_like_lua_literals() {
        assert_eq!(format_value(&ScriptValue::Nil), "nil");
        assert_eq!(format_value(&ScriptValue::Bool(true)), "true");
        assert_eq!(format_value(&ScriptValue::Int(3)), "3");
        assert_eq!(format_value(&ScriptValue::Number(2.0)), "2.0");
        assert_eq!(format_value(&ScriptValue::Number(0.5)), "0.5");
        assert_eq!(format_value(&ScriptValue::from("hi")), "\"hi\"");
        assert_eq!(
            format_value(&ScriptValue::List(vec![
                ScriptValue::Int(1),
                ScriptValue::from("a")
            ])),
            "{1, \"a\"}"
        );
        let map = BTreeMap::from([("k".to_owned(), ScriptValue::Bool(false))]);
        assert_eq!(format_value(&ScriptValue::Map(map)), "{k = false}");
    }

    #[test]
    fn numbers_compare_across_int_and_float() {
        assert!(values_equal(
            &ScriptValue::Int(3),
            &ScriptValue::Number(3.0)
        ));
        assert!(values_equal(
            &ScriptValue::Number(3.0),
            &ScriptValue::Int(3)
        ));
        assert!(!values_equal(
            &ScriptValue::Int(3),
            &ScriptValue::Number(3.5)
        ));
        assert!(!values_equal(&ScriptValue::Int(1), &ScriptValue::from("1")));
        assert!(values_equal(
            &ScriptValue::List(vec![ScriptValue::Int(1)]),
            &ScriptValue::List(vec![ScriptValue::Number(1.0)])
        ));
        assert!(!values_equal(
            &ScriptValue::List(vec![ScriptValue::Int(1)]),
            &ScriptValue::List(vec![])
        ));
    }

    #[test]
    fn placement_formats_with_optional_facing() {
        assert_eq!(format_placement(GridPos::new(1, 2), None), "(1, 2)");
        assert_eq!(
            format_placement(GridPos::new(1, 2), Some(Direction::Up)),
            "(1, 2) facing up"
        );
    }
}
