//! Engine-side vocabulary for what plugins register besides hooks (§9.4, §10): field actions
//! and input actions. The registrations themselves live in the script host
//! ([`LuaScriptHost`](crate::LuaScriptHost)); these types carry them to the engine.

use std::collections::BTreeMap;

use pkrs_core::{Direction, GridPos};

use crate::host::{ScriptOwner, ScriptValue};

/// Where a field action is used from (§9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldActionSource {
    /// Picked in the party menu.
    PartyMenu,
    /// The player pressed Confirm facing a tile of the action's terrain.
    FacingTile,
}

impl FieldActionSource {
    /// `"party"` or `"tile"` (the `ctx.source` scripts see).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::PartyMenu => "party",
            Self::FacingTile => "tile",
        }
    }
}

/// The situation a field action is offered or run in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldContext {
    /// Party menu or facing tile.
    pub source: FieldActionSource,
    /// Player tile.
    pub player: GridPos,
    /// Player facing.
    pub facing: Direction,
    /// Terrain of the tile in front of the player, if any.
    pub target_terrain: Option<String>,
}

impl FieldContext {
    /// The tile in front of the player.
    #[must_use]
    pub fn target(&self) -> GridPos {
        self.player.neighbour(self.facing)
    }

    /// `ctx` as scripts see it:
    /// `{ source = "party"|"tile", x, y, facing, target = { x, y, terrain } }`.
    #[must_use]
    pub fn to_value(&self) -> ScriptValue {
        let int = |n: i32| ScriptValue::Int(i64::from(n));
        let target = self.target();
        let mut t = BTreeMap::new();
        t.insert("x".to_owned(), int(target.x));
        t.insert("y".to_owned(), int(target.y));
        if let Some(terrain) = &self.target_terrain {
            t.insert("terrain".to_owned(), ScriptValue::from(terrain.as_str()));
        }
        let mut m = BTreeMap::new();
        m.insert("source".to_owned(), ScriptValue::from(self.source.name()));
        m.insert("x".to_owned(), int(self.player.x));
        m.insert("y".to_owned(), int(self.player.y));
        m.insert(
            "facing".to_owned(),
            ScriptValue::String(self.facing.to_string()),
        );
        m.insert("target".to_owned(), ScriptValue::Map(t));
        ScriptValue::Map(m)
    }
}

/// A field action registered with `world.register_field_action(id, def)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldActionInfo {
    /// Action id (`cutter:cut`).
    pub id: String,
    /// Party-menu label (text key); the id when not given.
    pub label: String,
    /// Who registered it.
    pub owner: ScriptOwner,
    /// Terrain the player must face to use it (`None`: usable anywhere).
    pub terrain: Option<String>,
    /// Listed in the party menu (`menu = false` hides it there).
    pub menu: bool,
}

/// Context for `world.register_item_use` handlers (field menu item on a party member).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemUseContext {
    /// Item id being used.
    pub item_id: String,
    /// Party index of the target creature.
    pub party_index: usize,
}

impl ItemUseContext {
    /// `ctx` as scripts see it: `{ item, party_index }`.
    #[must_use]
    pub fn to_value(&self) -> ScriptValue {
        let mut m = BTreeMap::new();
        m.insert("item".to_owned(), ScriptValue::from(self.item_id.as_str()));
        m.insert(
            "party_index".to_owned(),
            ScriptValue::Int(i64::try_from(self.party_index).unwrap_or(0)),
        );
        ScriptValue::Map(m)
    }
}

/// Metadata for a `world.register_item_use` hook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemUseInfo {
    /// Item id or `namespace:` prefix.
    pub id: String,
    /// Who registered it.
    pub owner: ScriptOwner,
}

/// An input action registered with `input.register_action(name, default_keys)` (§10). The
/// engine binds it in the `Input` facade; `pkrs.toml [input]` overrides the default keys.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputActionRequest {
    /// Who registered it.
    pub owner: ScriptOwner,
    /// Action name (`resonance:toggle`).
    pub name: String,
    /// Default key names (`"r"`, `"left_shift"`, …), resolved by the engine.
    pub keys: Vec<String>,
}
