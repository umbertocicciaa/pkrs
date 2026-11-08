//! [`SaveData`]: the typed save model (§12).

use std::collections::BTreeMap;

use pkrs_core::{Direction, GridPos};
use serde::{Deserialize, Serialize};

use crate::codec::RonCodec;
use crate::error::Result;
use crate::serde_value::{from_value, to_value};
use crate::value::Value;
use crate::world::WorldState;

/// The save schema version this build writes (§12). Older saves are migrated step by step
/// ([`Migrations`](crate::Migrations)).
pub const CURRENT_FORMAT_VERSION: u32 = 2;

/// RNG stream battles derive their seeds from (`RngPolicy::Local`, §7.2).
pub const RNG_BATTLE: &str = "battle";
/// RNG stream wild encounter rolls draw from (§6.3).
pub const RNG_ENCOUNTER: &str = "encounter";
/// RNG stream behind the scripts' `math.random` (§9.6).
pub const RNG_SCRIPT: &str = "script";

/// One save file: header, story state and plugin namespaces (§12).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SaveData {
    /// Version, game identity and slot-screen summary.
    pub header: SaveHeader,
    /// Story state, player, party and RNG streams.
    #[serde(default)]
    pub world: WorldState,
    /// Plugin save namespaces (`plugin.save`, §9.4), by plugin id.
    #[serde(default)]
    pub plugins: BTreeMap<String, Value>,
}

impl SaveData {
    /// The save as a [`Value`] tree.
    pub fn to_value(&self) -> Result<Value> {
        to_value(self)
    }

    /// The typed save of a (migrated) [`Value`] tree.
    pub fn from_value(value: Value) -> Result<Self> {
        from_value(value)
    }

    /// Pretty-printed RON text of the save.
    pub fn to_ron(&self) -> Result<String> {
        RonCodec::encode(&self.to_value()?)
    }

    /// Stores plugin `id`'s namespace after [`sanitize_plugin_value`]; a value with nothing
    /// saveable left removes the namespace.
    pub fn set_plugin(&mut self, id: &str, value: Value) {
        match sanitize_plugin_value(id, value) {
            Some(value) => {
                self.plugins.insert(id.to_owned(), value);
            }
            None => {
                self.plugins.remove(id);
            }
        }
    }
}

/// Version and identity of a save, plus what the slot screen shows (§12).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveHeader {
    /// Save schema version ([`CURRENT_FORMAT_VERSION`] when written by this build).
    pub format_version: u32,
    /// Engine version that wrote the save.
    #[serde(default)]
    pub engine_version: String,
    /// `game.id` of the game that wrote the save.
    #[serde(default)]
    pub game_id: String,
    /// `game.version` of the game that wrote the save (input of `scripts/migrate.lua`).
    #[serde(default)]
    pub game_version: String,
    /// When the save was written, in seconds since the Unix epoch.
    #[serde(default)]
    pub timestamp: u64,
    /// Total play time in simulation ticks.
    #[serde(default)]
    pub play_time_ticks: u64,
    /// Slot-screen summary.
    #[serde(default)]
    pub summary: SaveSummary,
}

impl Default for SaveHeader {
    fn default() -> Self {
        Self {
            format_version: CURRENT_FORMAT_VERSION,
            engine_version: String::new(),
            game_id: String::new(),
            game_version: String::new(),
            timestamp: 0,
            play_time_ticks: 0,
            summary: SaveSummary::default(),
        }
    }
}

/// What the slot screen shows for a save.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SaveSummary {
    /// Player name.
    pub player_name: String,
    /// Where the player is (map id).
    pub location: String,
    /// Progress count (badges or another game-defined counter).
    pub progress: u32,
    /// Party size.
    pub party_size: u32,
}

/// Where the player stands in a save.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerPlacement {
    /// Map id.
    pub map: String,
    /// Tile column.
    pub x: i32,
    /// Tile row.
    pub y: i32,
    /// Facing (`up`, `down`, `left`, `right`).
    #[serde(with = "direction_repr")]
    pub facing: Direction,
    /// Field states (`surf`, …) that were on.
    #[serde(default)]
    pub field_states: BTreeMap<String, bool>,
}

impl PlayerPlacement {
    /// The player on `map` at `pos` facing `facing`, with no field state.
    #[must_use]
    pub fn new(map: impl Into<String>, pos: GridPos, facing: Direction) -> Self {
        Self {
            map: map.into(),
            x: pos.x,
            y: pos.y,
            facing,
            field_states: BTreeMap::new(),
        }
    }

    /// The tile.
    #[must_use]
    pub fn pos(&self) -> GridPos {
        GridPos::new(self.x, self.y)
    }
}

mod direction_repr {
    use pkrs_core::Direction;
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S: Serializer>(dir: &Direction, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&dir.to_string())
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Direction, D::Error> {
        String::deserialize(d)?.parse().map_err(D::Error::custom)
    }
}

/// One party member in a save (§7.3): species, level, total exp and known moves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedCreature {
    /// Species id.
    pub species: String,
    /// Level.
    pub level: u8,
    /// Total experience.
    #[serde(default)]
    pub exp: u32,
    /// Known moves, in slot order.
    #[serde(default)]
    pub moves: Vec<String>,
}

/// Drops what cannot be written to a save from plugin `plugin`'s namespace and logs every key
/// it drops (§9.6): non-finite numbers (`NaN`, `inf`), which RON cannot round-trip. Returns
/// `None` when nothing saveable is left.
///
/// Script-only values (functions, userdata, NPC handles, cycles) never reach a [`Value`]: the
/// script host skips and logs them when it snapshots `plugin.save` (§9.4).
#[must_use]
pub fn sanitize_plugin_value(plugin: &str, value: Value) -> Option<Value> {
    sanitize(plugin, value, "plugin.save")
}

fn sanitize(plugin: &str, value: Value, path: &str) -> Option<Value> {
    match value {
        Value::Float(f) if !f.is_finite() => {
            tracing::warn!(target: "save", plugin, key = path, "plugin save: skipped non-finite number {f}");
            None
        }
        Value::List(items) => Some(Value::List(
            items
                .into_iter()
                .enumerate()
                .filter_map(|(i, v)| sanitize(plugin, v, &format!("{path}[{}]", i + 1)))
                .collect(),
        )),
        Value::Map(map) => Some(Value::Map(
            map.into_iter()
                .filter_map(|(k, v)| {
                    let child = format!("{path}.{k}");
                    sanitize(plugin, v, &child).map(|v| (k, v))
                })
                .collect(),
        )),
        other => Some(other),
    }
}
