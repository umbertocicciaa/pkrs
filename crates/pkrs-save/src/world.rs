//! [`WorldState`] and per-map [`MapState`]: the story progression a save carries (§6.5, §12).
//!
//! The overworld keeps one `WorldState` for the whole session. Engine events consult it before
//! running scripts (a fired `once` trigger stays silent, a picked-up item is not spawned again)
//! and it survives leaving and re-entering maps. It is also the `world` part of
//! [`SaveData`](crate::SaveData): the session pieces the runtime keeps elsewhere during play
//! (player position, party, RNG streams) are copied into it when a snapshot is taken (§12).
//!
//! Map state is keyed by **map id**, then by the object's **persistent key**: its `uid`
//! property if set, otherwise its Tiled object id (`pkrs_world::MapObject::persistent_key`).

use std::collections::{BTreeMap, BTreeSet};

use pkrs_core::{Direction, GridPos, RngState};
use serde::{Deserialize, Serialize};

use crate::model::{PlayerPlacement, SavedCreature};
use crate::value::Value;

/// Persistent state of one map object (NPC, item, sign, trigger) that differs from the map file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ObjectState {
    /// Hidden (`npc:hide()`); hidden objects are not drawn, block nothing and cannot be
    /// interacted with or triggered.
    #[serde(skip_serializing_if = "is_false")]
    pub hidden: bool,
    /// The item was picked up; the item ball is not spawned again.
    #[serde(skip_serializing_if = "is_false")]
    pub picked_up: bool,
    /// Where the object stands now when a script moved it away from its map position.
    #[serde(with = "tile_repr")]
    pub moved_to: Option<(GridPos, Direction)>,
}

fn is_false(b: &bool) -> bool {
    !*b
}

/// Saved form of a tile and facing: `{ "x": 1, "y": 2, "facing": "up" }`.
pub(crate) mod tile_repr {
    use pkrs_core::{Direction, GridPos};
    use serde::de::Error as _;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct Tile {
        x: i32,
        y: i32,
        facing: String,
    }

    pub(crate) fn serialize<S: Serializer>(
        value: &Option<(GridPos, Direction)>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        value
            .map(|(pos, facing)| Tile {
                x: pos.x,
                y: pos.y,
                facing: facing.to_string(),
            })
            .serialize(serializer)
    }

    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<(GridPos, Direction)>, D::Error> {
        let Some(tile) = Option::<Tile>::deserialize(deserializer)? else {
            return Ok(None);
        };
        let facing = tile.facing.parse::<Direction>().map_err(D::Error::custom)?;
        Ok(Some((GridPos::new(tile.x, tile.y), facing)))
    }
}

impl ObjectState {
    /// `true` when nothing differs from the map file.
    #[must_use]
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Persistent state of one map (§6.5): object state, fired `once` triggers and map variables.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MapState {
    objects: BTreeMap<String, ObjectState>,
    fired: BTreeSet<String>,
    vars: BTreeMap<String, Value>,
}

impl MapState {
    /// Empty state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// State of the object with persistent key `key` (default when never changed).
    #[must_use]
    pub fn object(&self, key: &str) -> ObjectState {
        self.objects.get(key).copied().unwrap_or_default()
    }

    /// Replaces the state of `key`; a default state removes the entry.
    pub fn set_object(&mut self, key: &str, state: ObjectState) {
        if state.is_default() {
            self.objects.remove(key);
        } else {
            self.objects.insert(key.to_owned(), state);
        }
    }

    /// Changes the state of `key` in place.
    pub fn update_object(&mut self, key: &str, change: impl FnOnce(&mut ObjectState)) {
        let mut state = self.object(key);
        change(&mut state);
        self.set_object(key, state);
    }

    /// Every object with non-default state, by persistent key.
    pub fn objects(&self) -> impl Iterator<Item = (&str, &ObjectState)> {
        self.objects.iter().map(|(k, v)| (k.as_str(), v))
    }

    /// `true` once the `once` trigger `key` fired.
    #[must_use]
    pub fn has_fired(&self, key: &str) -> bool {
        self.fired.contains(key)
    }

    /// Records that the `once` trigger `key` fires now. Returns `false` (and records nothing)
    /// when it already fired, so callers run `on_enter` only on `true`.
    pub fn fire_once(&mut self, key: &str) -> bool {
        self.fired.insert(key.to_owned())
    }

    /// Keys of the `once` triggers that fired.
    pub fn fired(&self) -> impl Iterator<Item = &str> {
        self.fired.iter().map(String::as_str)
    }

    /// Map variable `name` (`map.var`).
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    /// Sets map variable `name` (`map.set_var`); `None` removes it.
    pub fn set_var(&mut self, name: &str, value: Option<Value>) {
        match value {
            Some(v) => {
                self.vars.insert(name.to_owned(), v);
            }
            None => {
                self.vars.remove(name);
            }
        }
    }

    /// Every map variable.
    #[must_use]
    pub fn vars(&self) -> &BTreeMap<String, Value> {
        &self.vars
    }

    /// `true` when the map has no persistent state at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.objects.is_empty() && self.fired.is_empty() && self.vars.is_empty()
    }
}

/// Session-wide story state (§6.5, §12): flags, variables, bag, money and per-map state, plus
/// the player's position, party, RNG streams and save-owned options of a snapshot.
///
/// During play the runtime owns the live player position, party and RNG streams; it writes
/// them into [`player`](Self::player), [`party`](Self::party) and [`rng`](Self::rng) when it
/// takes a save snapshot and reads them back when it restores one. PC boxes join this model
/// with creature storage.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldState {
    flags: BTreeMap<String, bool>,
    vars: BTreeMap<String, Value>,
    bag: BTreeMap<String, u32>,
    /// Money.
    pub money: i64,
    maps: BTreeMap<String, MapState>,
    /// Current map, tile, facing and field states (`None` until a snapshot sets it).
    pub player: Option<PlayerPlacement>,
    /// The player's party, in party order.
    pub party: Vec<SavedCreature>,
    /// RNG streams by name ([`RNG_BATTLE`](crate::RNG_BATTLE), …).
    pub rng: BTreeMap<String, RngState>,
    /// Text language chosen in this save (`None`: the game's default language).
    pub language: Option<String>,
}

impl WorldState {
    /// Empty state.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Flag `name` (`false` when never set).
    #[must_use]
    pub fn flag(&self, name: &str) -> bool {
        self.flags.get(name).copied().unwrap_or(false)
    }

    /// Sets flag `name`.
    pub fn set_flag(&mut self, name: &str, value: bool) {
        self.flags.insert(name.to_owned(), value);
    }

    /// Every flag that was set, with its value.
    #[must_use]
    pub fn flags(&self) -> &BTreeMap<String, bool> {
        &self.flags
    }

    /// Variable `name`.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&Value> {
        self.vars.get(name)
    }

    /// Sets variable `name`; `None` removes it.
    pub fn set_var(&mut self, name: &str, value: Option<Value>) {
        match value {
            Some(v) => {
                self.vars.insert(name.to_owned(), v);
            }
            None => {
                self.vars.remove(name);
            }
        }
    }

    /// Every variable.
    #[must_use]
    pub fn vars(&self) -> &BTreeMap<String, Value> {
        &self.vars
    }

    /// Adds `count` of `item` to the bag (saturating).
    pub fn give_item(&mut self, item: &str, count: u32) {
        let slot = self.bag.entry(item.to_owned()).or_default();
        *slot = slot.saturating_add(count);
    }

    /// How many of `item` the bag holds.
    #[must_use]
    pub fn item_count(&self, item: &str) -> u32 {
        self.bag.get(item).copied().unwrap_or(0)
    }

    /// The bag, by item id.
    #[must_use]
    pub fn bag(&self) -> &BTreeMap<String, u32> {
        &self.bag
    }

    /// Replaces the bag (field menu commit, save restore).
    pub fn replace_bag(&mut self, bag: BTreeMap<String, u32>) {
        self.bag = bag;
    }

    /// State of map `map_id`, if it has any.
    #[must_use]
    pub fn map(&self, map_id: &str) -> Option<&MapState> {
        self.maps.get(map_id)
    }

    /// State of map `map_id`, created empty on first use.
    pub fn map_mut(&mut self, map_id: &str) -> &mut MapState {
        self.maps.entry(map_id.to_owned()).or_default()
    }

    /// Every map with state, by map id.
    pub fn maps(&self) -> impl Iterator<Item = (&str, &MapState)> {
        self.maps.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn map_state_is_keyed_by_map_id_then_object_key() {
        let mut world = WorldState::new();
        world
            .map_mut("route1")
            .update_object("7", |o| o.hidden = true);
        world
            .map_mut("route2")
            .update_object("lamp_oil", |o| o.picked_up = true);
        assert!(world.map("route1").unwrap().object("7").hidden);
        assert!(!world.map("route1").unwrap().object("lamp_oil").picked_up);
        assert!(world.map("route2").unwrap().object("lamp_oil").picked_up);
        assert!(!world.map("route2").unwrap().object("7").hidden);
        assert!(world.map("town").is_none());
        let maps: Vec<&str> = world.maps().map(|(id, _)| id).collect();
        assert_eq!(maps, ["route1", "route2"]);
    }

    #[test]
    fn default_object_state_is_not_stored() {
        let mut map = MapState::new();
        map.update_object("1", |o| o.hidden = true);
        assert_eq!(map.objects().count(), 1);
        map.update_object("1", |o| o.hidden = false);
        assert_eq!(map.objects().count(), 0);
        assert!(map.is_empty());
        map.set_object(
            "2",
            ObjectState {
                moved_to: Some((GridPos::new(1, 2), Direction::Left)),
                ..ObjectState::default()
            },
        );
        assert_eq!(
            map.object("2").moved_to,
            Some((GridPos::new(1, 2), Direction::Left))
        );
    }

    #[test]
    fn once_triggers_fire_exactly_once_per_map() {
        let mut world = WorldState::new();
        assert!(world.map_mut("town").fire_once("rival"));
        assert!(!world.map_mut("town").fire_once("rival"));
        assert!(world.map("town").unwrap().has_fired("rival"));
        // Same key on another map is a different trigger.
        assert!(world.map_mut("cave").fire_once("rival"));
        let fired: Vec<&str> = world.map("town").unwrap().fired().collect();
        assert_eq!(fired, ["rival"]);
    }

    #[test]
    fn variables_and_flags() {
        let mut world = WorldState::new();
        assert!(!world.flag("met"));
        world.set_flag("met", true);
        assert!(world.flag("met"));
        world.set_var("count", Some(Value::Int(2)));
        assert_eq!(world.var("count"), Some(&Value::Int(2)));
        world.set_var("count", None);
        assert_eq!(world.var("count"), None);
        world
            .map_mut("town")
            .set_var("visits", Some(Value::from(3)));
        assert_eq!(
            world.map("town").unwrap().var("visits"),
            Some(&Value::Int(3))
        );
        assert_eq!(world.map_mut("cave").var("visits"), None);
        world.give_item("potion", 2);
        world.give_item("potion", u32::MAX);
        assert_eq!(world.item_count("potion"), u32::MAX);
    }
}
