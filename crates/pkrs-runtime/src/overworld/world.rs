//! The runtime's [`ScriptWorld`] adapter: overworld state scripts can read and change (§9.1).
//!
//! [`WorldModel`] holds the session's [`WorldState`] (flags, variables, bag, money and the
//! per-map [`MapState`](pkrs_save::MapState), §6.5), the entities spawned from the active map's
//! objects (NPCs, signs, items, triggers) and the requests scripts made this tick (dialogue,
//! warp, camera). The script host gets a [`SharedWorld`] handle to it; the overworld reads the
//! requests back after the scripts and events phases.
//!
//! **Persistent map state.** Entities are spawned from the map file *through* the map's
//! `MapState`: hidden objects spawn hidden (NPCs) or not at all (items, signs, triggers),
//! picked-up items are skipped and moved NPCs stand where they were left. NPC changes are
//! written back by [`WorldModel::sync_map_state`], which runs when the player leaves the map.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use pkrs_core::is_action_name;
use pkrs_core::{Direction, GridPos};
use pkrs_data::{Localizer, TextArgs};
use pkrs_render::UniformValue;
use pkrs_save::WorldState;
use pkrs_script::{ApiError, ApiResult, CameraCommand, ScriptValue, ScriptWorld, TileInfo};
use pkrs_world::{
    FieldState, GridStepper, LogicalGrid, Map, MapObject, MovementConfig, ObjectKind, PathProgress,
    ScriptedPath, TerrainRules, TileRect,
};

/// A dialogue or choice a script asked for (`ui.say`, `ui.choose`, `ui.yes_no`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UiRequest {
    /// A text box closed with Confirm.
    Say(String),
    /// A choice list; answers with the 1-based index or `nil` on cancel.
    Choose {
        /// Text shown above the options (`ui.yes_no`).
        prompt: Option<String>,
        /// Option labels.
        options: Vec<String>,
    },
}

/// A warp a script asked for (`map.warp`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarpRequest {
    /// Target map id.
    pub map: String,
    /// Target tile.
    pub to: GridPos,
    /// Facing on arrival.
    pub facing: Option<Direction>,
}

/// One NPC of the active map.
#[derive(Clone, Debug)]
pub struct NpcState {
    /// Script id (the object's name, or its Tiled id when unnamed).
    pub id: String,
    /// Persistent key in the map's `MapState` (`uid` property, else the Tiled id; §6.5).
    pub key: String,
    /// Tiled object id (scene entity id).
    pub object_id: u32,
    /// Tile the map file places the NPC on.
    pub home: GridPos,
    /// Grid movement.
    pub walker: GridStepper,
    /// Scripted path in progress (`npc:move`).
    pub path: Option<ScriptedPath>,
    /// Sprite id (`npc:set_sprite`).
    pub sprite: Option<String>,
    /// `false` after `npc:hide()`.
    pub visible: bool,
    /// Interaction script reference (`maps/route1.lua:old_man`).
    pub script: Option<String>,
}

/// What a non-NPC map entity does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntityKind {
    /// Readable sign.
    Sign {
        /// Interaction script reference.
        script: Option<String>,
        /// Text key shown when there is no script.
        text: Option<String>,
    },
    /// Item ball: picked up with Confirm, then gone for good (§6.5).
    Item {
        /// Item id.
        item: String,
        /// How many.
        count: u32,
    },
    /// Area trigger.
    Trigger {
        /// Script started when the player steps in.
        on_enter: Option<String>,
        /// Fire at most once (recorded in the map's `MapState`).
        once: bool,
    },
}

/// A sign, item or trigger spawned from the active map's objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEntity {
    /// Script-facing id (the object's name, or its persistent key when unnamed).
    pub id: String,
    /// Persistent key in the map's `MapState`.
    pub key: String,
    /// Tiled object id (scene entity id for items).
    pub object_id: u32,
    /// Tiles covered.
    pub rect: TileRect,
    /// What it does.
    pub kind: EntityKind,
}

/// What Confirm acts on: the first match on the tile the player faces, in this order (§6.2):
/// a visible NPC standing there, an item ball lying there, a sign covering it, else the tile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InteractionTarget {
    /// Talk to the NPC with this script id.
    Npc {
        /// Script id.
        id: String,
        /// Its interaction script.
        script: Option<String>,
    },
    /// Pick up an item ball.
    Item {
        /// Persistent key.
        key: String,
        /// Item id.
        item: String,
        /// How many.
        count: u32,
    },
    /// Read a sign.
    Sign {
        /// Persistent key.
        key: String,
        /// Interaction script.
        script: Option<String>,
        /// Text key.
        text: Option<String>,
    },
    /// Nothing there: the plain tile.
    Tile,
}

/// Overworld state behind the [`ScriptWorld`] port.
#[derive(Debug)]
pub struct WorldModel {
    /// Flags, variables, bag, money and per-map persistent state (§6.5). Survives map changes.
    pub state: WorldState,
    /// Active map id.
    pub map_id: Option<String>,
    grid: Option<LogicalGrid>,
    /// Player tile.
    pub player_pos: GridPos,
    /// Player facing.
    pub player_facing: Direction,
    /// Facing a script asked for (`player.face`), applied by the overworld.
    pub player_face_request: Option<Direction>,
    /// `player.lock()` is in effect.
    pub script_lock: bool,
    /// Field states (`surf`, …).
    pub field_states: BTreeMap<String, bool>,
    /// NPCs of the active map (hidden ones included, so `npc:show()` works).
    pub npcs: Vec<NpcState>,
    /// Signs, items and triggers of the active map that are still present.
    pub entities: Vec<MapEntity>,
    /// Scene entities to remove (picked-up items), drained by the overworld.
    pub removed_entities: Vec<u32>,
    /// Dialogue requests, oldest first.
    pub ui_requests: VecDeque<UiRequest>,
    /// `ui.notify` messages not yet shown.
    pub notifications: Vec<String>,
    /// Pending `map.warp`.
    pub warp_request: Option<WarpRequest>,
    /// Camera commands, in call order.
    pub camera: Vec<CameraCommand>,
    /// `render.set_global_uniform` calls not yet applied to the renderer, in call order.
    pub global_uniforms: Vec<(String, UniformValue)>,
    /// Active language and text tables (§5.5): dialogue, choices and notifications are
    /// localized with it when they are queued.
    pub localizer: Localizer,
}

impl Default for WorldModel {
    fn default() -> Self {
        Self {
            state: WorldState::new(),
            map_id: None,
            grid: None,
            player_pos: GridPos::new(0, 0),
            player_facing: Direction::Down,
            player_face_request: None,
            script_lock: false,
            field_states: BTreeMap::new(),
            npcs: Vec::new(),
            entities: Vec::new(),
            removed_entities: Vec::new(),
            ui_requests: VecDeque::new(),
            notifications: Vec::new(),
            warp_request: None,
            camera: Vec::new(),
            global_uniforms: Vec::new(),
            localizer: crate::locale::engine_localizer(),
        }
    }
}

/// Built-in field states `player.set_field_state` accepts. Plugins may also use any
/// namespaced state (`plugin_id:state`), e.g. one granted by a field action (§9.4).
pub const FIELD_STATES: &[&str] = &["surf"];

/// `true` for a state `player.set_field_state` accepts.
#[must_use]
pub fn is_field_state(name: &str) -> bool {
    FIELD_STATES.contains(&name) || (name.contains(':') && is_action_name(name))
}

/// Script-facing id of an object: its name, else `fallback`.
fn object_id(object: &MapObject, fallback: String) -> String {
    if object.name.is_empty() {
        fallback
    } else {
        object.name.clone()
    }
}

impl WorldModel {
    /// Queues a text box showing `text` (literal or `text:` key) localized with `args`.
    pub fn say(&mut self, text: &str, args: &TextArgs) {
        let shown = self.localizer.text(text, args);
        self.ui_requests.push_back(UiRequest::Say(shown));
    }

    /// Replaces the map-dependent state with `map`'s entities and grid, the player standing on
    /// `player` facing `facing`. Entities spawn through the map's persistent state (§6.5).
    pub fn enter_map(&mut self, map: &Map, player: GridPos, facing: Direction) {
        self.map_id = Some(map.id.clone());
        self.grid = Some(map.grid.clone());
        self.player_pos = player;
        self.player_facing = facing;
        self.warp_request = None;
        self.removed_entities.clear();
        let config = MovementConfig {
            tap_turn_ticks: 1,
            ..MovementConfig::default()
        };
        let saved = self.state.map(&map.id);
        let mut npcs = Vec::new();
        let mut entities = Vec::new();
        for o in &map.objects {
            let key = o.persistent_key();
            let persisted = saved.map(|m| m.object(&key)).unwrap_or_default();
            let kind = match &o.kind {
                ObjectKind::Npc {
                    script,
                    sprite,
                    facing,
                } => {
                    let (pos, facing) = persisted.moved_to.unwrap_or((o.pos(), *facing));
                    npcs.push(NpcState {
                        id: object_id(o, o.id.to_string()),
                        key,
                        object_id: o.id,
                        home: o.pos(),
                        walker: GridStepper::with_config(pos, facing, config),
                        path: None,
                        sprite: sprite.clone(),
                        visible: !persisted.hidden,
                        script: script.clone(),
                    });
                    continue;
                }
                _ if persisted.hidden => continue,
                ObjectKind::Sign { script, text } => EntityKind::Sign {
                    script: script.clone(),
                    text: text.clone(),
                },
                ObjectKind::Item { .. } if persisted.picked_up => continue,
                ObjectKind::Item { item, count } => EntityKind::Item {
                    item: item.clone(),
                    count: *count,
                },
                ObjectKind::Trigger { on_enter, once } => EntityKind::Trigger {
                    on_enter: on_enter.clone(),
                    once: *once,
                },
                _ => continue,
            };
            entities.push(MapEntity {
                id: object_id(o, key.clone()),
                key,
                object_id: o.id,
                rect: o.rect,
                kind,
            });
        }
        self.npcs = npcs;
        self.entities = entities;
    }

    /// Writes the active map's NPC changes into its `MapState`: hidden NPCs stay hidden and
    /// NPCs a script moved off their tile stay there (with their facing). An NPC standing on
    /// its map tile keeps no state, so it faces its map direction again next time. Runs when
    /// the player leaves the map; call it before taking a save snapshot too.
    pub fn sync_map_state(&mut self) {
        let Some(map_id) = self.map_id.clone() else {
            return;
        };
        let map = self.state.map_mut(&map_id);
        for npc in &self.npcs {
            let pos = npc.walker.pos();
            map.update_object(&npc.key, |o| {
                o.hidden = !npc.visible;
                o.moved_to = (pos != npc.home).then(|| (pos, npc.walker.facing()));
            });
        }
    }

    /// The active field states as the movement rules see them (§6.3).
    #[must_use]
    pub fn field_state(&self) -> FieldState {
        let mut field = FieldState::default();
        for (name, on) in &self.field_states {
            field.set(name, *on);
        }
        field
    }

    /// Terrain name of tile `pos` on the active map, if any.
    #[must_use]
    pub fn terrain_at(&self, pos: GridPos) -> Option<String> {
        let cell = self.grid.as_ref()?.cell(pos)?;
        cell.terrain.as_ref().map(|t| t.name().to_owned())
    }

    /// The NPC with script id `id`.
    #[must_use]
    pub fn npc(&self, id: &str) -> Option<&NpcState> {
        self.npcs.iter().find(|n| n.id == id)
    }

    fn npc_mut(&mut self, id: &str) -> ApiResult<&mut NpcState> {
        let map = self.map_id.clone().unwrap_or_default();
        self.npcs
            .iter_mut()
            .find(|n| n.id == id)
            .ok_or_else(|| ApiError::new(format!("no npc `{id}` on map `{map}`")))
    }

    /// The visible NPC standing on `pos`.
    #[must_use]
    pub fn npc_at(&self, pos: GridPos) -> Option<&NpcState> {
        self.npcs
            .iter()
            .find(|n| n.visible && n.walker.pos() == pos)
    }

    /// The item ball lying on `pos`.
    #[must_use]
    pub fn item_at(&self, pos: GridPos) -> Option<&MapEntity> {
        self.entities
            .iter()
            .find(|e| matches!(e.kind, EntityKind::Item { .. }) && e.rect.contains(pos))
    }

    /// `true` when a visible NPC or an item ball occupies `pos` (player occupancy).
    #[must_use]
    pub fn blocks(&self, pos: GridPos) -> bool {
        self.npc_at(pos).is_some() || self.item_at(pos).is_some()
    }

    /// The tile the player faces.
    #[must_use]
    pub fn facing_tile(&self) -> GridPos {
        self.player_pos.neighbour(self.player_facing)
    }

    /// What Confirm acts on right now (see [`InteractionTarget`] for the order).
    #[must_use]
    pub fn interaction_target(&self) -> InteractionTarget {
        let tile = self.facing_tile();
        if let Some(npc) = self.npc_at(tile) {
            return InteractionTarget::Npc {
                id: npc.id.clone(),
                script: npc.script.clone(),
            };
        }
        if let Some(MapEntity {
            key,
            kind: EntityKind::Item { item, count },
            ..
        }) = self.item_at(tile)
        {
            return InteractionTarget::Item {
                key: key.clone(),
                item: item.clone(),
                count: *count,
            };
        }
        let sign = self.entities.iter().find_map(|e| match &e.kind {
            EntityKind::Sign { script, text } if e.rect.contains(tile) => {
                Some(InteractionTarget::Sign {
                    key: e.key.clone(),
                    script: script.clone(),
                    text: text.clone(),
                })
            }
            _ => None,
        });
        sign.unwrap_or(InteractionTarget::Tile)
    }

    /// Picks up the item ball with persistent key `key`: adds it to the bag, records it as
    /// picked up in the map's state and despawns it. `false` when there is no such item.
    pub fn pick_up_item(&mut self, key: &str) -> bool {
        let Some(map_id) = self.map_id.clone() else {
            return false;
        };
        let Some(index) = self
            .entities
            .iter()
            .position(|e| e.key == key && matches!(e.kind, EntityKind::Item { .. }))
        else {
            return false;
        };
        let entity = self.entities.remove(index);
        if let EntityKind::Item { item, count } = &entity.kind {
            self.state.give_item(item, *count);
            tracing::info!(target: "script", map = %map_id, key, item, count, "item picked up");
        }
        self.state
            .map_mut(&map_id)
            .update_object(key, |o| o.picked_up = true);
        self.removed_entities.push(entity.object_id);
        true
    }

    /// Triggers the player enters by stepping from `from` to `to`, in map order. `once`
    /// triggers are recorded as fired in the map's state and are left out when they already
    /// fired (§6.5). Returns `(id, on_enter)` for each.
    pub fn enter_triggers(&mut self, from: GridPos, to: GridPos) -> Vec<(String, Option<String>)> {
        let Some(map_id) = self.map_id.clone() else {
            return Vec::new();
        };
        let mut entered = Vec::new();
        for entity in &self.entities {
            let EntityKind::Trigger { on_enter, once } = &entity.kind else {
                continue;
            };
            if !entity.rect.contains(to) || entity.rect.contains(from) {
                continue;
            }
            if *once && !self.state.map_mut(&map_id).fire_once(&entity.key) {
                continue;
            }
            entered.push((entity.id.clone(), on_enter.clone()));
        }
        entered
    }

    /// Advances every NPC on a scripted path by one tick; returns the ids whose path ended.
    pub fn tick_npcs(&mut self, rules: &dyn TerrainRules) -> Vec<String> {
        let Some(grid) = self.grid.as_ref() else {
            return Vec::new();
        };
        let player = self.player_pos;
        let items: Vec<TileRect> = self
            .entities
            .iter()
            .filter(|e| matches!(e.kind, EntityKind::Item { .. }))
            .map(|e| e.rect)
            .collect();
        let mut occupied: Vec<Option<GridPos>> = self
            .npcs
            .iter()
            .map(|n| n.visible.then(|| n.walker.pos()))
            .collect();
        let mut finished = Vec::new();
        for (i, npc) in self.npcs.iter_mut().enumerate() {
            let Some(path) = npc.path.as_mut() else {
                continue;
            };
            let others = |p: GridPos| {
                p == player
                    || items.iter().any(|r| r.contains(p))
                    || occupied
                        .iter()
                        .enumerate()
                        .any(|(j, o)| j != i && *o == Some(p))
            };
            let progress = path.tick(&mut npc.walker, grid, &others, rules);
            occupied[i] = npc.visible.then(|| npc.walker.pos());
            if progress.is_done() {
                if progress == PathProgress::GaveUp {
                    tracing::warn!(target: "script", npc = %npc.id, "npc:move gave up: path blocked");
                }
                npc.path = None;
                finished.push(npc.id.clone());
            }
        }
        finished
    }
}

/// Shared handle to a [`WorldModel`]: one copy goes to the script host, one stays with the
/// overworld. Never borrowed across a script call.
#[derive(Clone, Debug, Default)]
pub struct SharedWorld(Rc<RefCell<WorldModel>>);

impl SharedWorld {
    /// Fresh world.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads the model.
    #[must_use]
    pub fn get(&self) -> Ref<'_, WorldModel> {
        self.0.borrow()
    }

    /// Changes the model.
    #[must_use]
    pub fn get_mut(&self) -> RefMut<'_, WorldModel> {
        self.0.borrow_mut()
    }
}

impl ScriptWorld for SharedWorld {
    fn flag(&self, name: &str) -> bool {
        self.get().state.flag(name)
    }

    fn set_flag(&mut self, name: &str, value: bool) {
        self.get_mut().state.set_flag(name, value);
    }

    fn var(&self, name: &str) -> ScriptValue {
        ScriptValue::from_save_value(self.get().state.var(name))
    }

    fn set_var(&mut self, name: &str, value: ScriptValue) {
        self.get_mut().state.set_var(name, value.to_save_value());
    }

    fn give_item(&mut self, item: &str, count: u32) -> ApiResult<()> {
        self.get_mut().state.give_item(item, count);
        tracing::info!(target: "script", item, count, "give_item");
        Ok(())
    }

    fn bag(&self) -> Vec<(String, u32)> {
        self.get()
            .state
            .bag()
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    fn money(&self) -> i64 {
        self.get().state.money
    }

    fn current_map(&self) -> Option<String> {
        self.get().map_id.clone()
    }

    fn warp(&mut self, map: &str, to: GridPos, facing: Option<Direction>) -> ApiResult<()> {
        if to.x < 0 || to.y < 0 {
            return Err(ApiError::new(format!("tile {to} is outside every map")));
        }
        self.get_mut().warp_request = Some(WarpRequest {
            map: map.to_owned(),
            to,
            facing,
        });
        Ok(())
    }

    fn tile(&self, pos: GridPos) -> Option<TileInfo> {
        let w = self.get();
        let cell = w.grid.as_ref()?.cell(pos)?;
        Some(TileInfo {
            solid: cell.solid,
            terrain: cell.terrain.as_ref().map(|t| t.name().to_owned()),
        })
    }

    fn map_var(&self, name: &str) -> ScriptValue {
        let w = self.get();
        let value = w
            .map_id
            .as_deref()
            .and_then(|id| w.state.map(id))
            .and_then(|m| m.var(name));
        ScriptValue::from_save_value(value)
    }

    fn set_map_var(&mut self, name: &str, value: ScriptValue) {
        let mut w = self.get_mut();
        let Some(map) = w.map_id.clone() else {
            tracing::warn!(target: "script", name, "map.set_var ignored: no map is loaded");
            return;
        };
        w.state.map_mut(&map).set_var(name, value.to_save_value());
    }

    fn npc_exists(&self, id: &str) -> bool {
        self.get().npc(id).is_some()
    }

    fn npc_pos(&self, id: &str) -> ApiResult<(GridPos, Direction)> {
        let w = self.get();
        let npc = w
            .npc(id)
            .ok_or_else(|| ApiError::new(format!("no npc `{id}` on this map")))?;
        Ok((npc.walker.pos(), npc.walker.facing()))
    }

    fn npc_face(&mut self, id: &str, dir: Direction) -> ApiResult<()> {
        let mut w = self.get_mut();
        let npc = w.npc_mut(id)?;
        if npc.walker.face(dir) {
            Ok(())
        } else {
            Err(ApiError::new(format!("npc `{id}` is walking")))
        }
    }

    fn npc_move(&mut self, id: &str, path: &[Direction]) -> ApiResult<()> {
        let mut w = self.get_mut();
        let npc = w.npc_mut(id)?;
        if npc.path.is_some() {
            return Err(ApiError::new(format!(
                "npc `{id}` is already walking a path"
            )));
        }
        npc.path = Some(ScriptedPath::new(path.iter().copied()));
        Ok(())
    }

    fn npc_set_sprite(&mut self, id: &str, sprite: &str) -> ApiResult<()> {
        let mut w = self.get_mut();
        w.npc_mut(id)?.sprite = Some(sprite.to_owned());
        Ok(())
    }

    fn npc_set_visible(&mut self, id: &str, visible: bool) -> ApiResult<()> {
        let mut w = self.get_mut();
        w.npc_mut(id)?.visible = visible;
        Ok(())
    }

    fn player_pos(&self) -> (GridPos, Direction) {
        let w = self.get();
        (w.player_pos, w.player_facing)
    }

    fn player_face(&mut self, dir: Direction) {
        let mut w = self.get_mut();
        w.player_facing = dir;
        w.player_face_request = Some(dir);
    }

    fn set_player_locked(&mut self, locked: bool) {
        self.get_mut().script_lock = locked;
    }

    fn set_field_state(&mut self, name: &str, value: bool) -> ApiResult<()> {
        if !is_field_state(name) {
            return Err(ApiError::new(format!(
                "unknown field state `{name}` (known: {}; plugins use `plugin_id:state`)",
                FIELD_STATES.join(", ")
            )));
        }
        self.get_mut().field_states.insert(name.to_owned(), value);
        Ok(())
    }

    fn say(&mut self, text: &str, args: &TextArgs) -> ApiResult<()> {
        self.get_mut().say(text, args);
        Ok(())
    }

    fn choose(
        &mut self,
        prompt: Option<&str>,
        args: &TextArgs,
        options: &[String],
    ) -> ApiResult<()> {
        let mut world = self.get_mut();
        let none = TextArgs::new();
        let prompt = prompt.map(|p| world.localizer.text(p, args));
        let options = options
            .iter()
            .map(|o| world.localizer.text(o, &none))
            .collect();
        world
            .ui_requests
            .push_back(UiRequest::Choose { prompt, options });
        Ok(())
    }

    fn notify(&mut self, text: &str, args: &TextArgs) {
        let mut world = self.get_mut();
        let text = world.localizer.text(text, args);
        tracing::info!(target: "script", text, "notify");
        world.notifications.push(text);
    }

    fn language(&self) -> String {
        self.get().localizer.language().to_owned()
    }

    fn languages(&self) -> Vec<String> {
        self.get().localizer.available().to_vec()
    }

    fn set_language(&mut self, language: &str) -> ApiResult<()> {
        self.get_mut()
            .localizer
            .set_language(language)
            .map_err(|e| ApiError::new(e.to_string()))?;
        tracing::info!(target: "script", language, "text language switched");
        Ok(())
    }

    fn text(&self, value: &str, args: &TextArgs) -> String {
        self.get().localizer.text(value, args)
    }

    fn camera(&mut self, command: CameraCommand) -> ApiResult<()> {
        self.get_mut().camera.push(command);
        Ok(())
    }

    fn set_global_uniform(&mut self, name: &str, value: UniformValue) -> ApiResult<()> {
        self.get_mut()
            .global_uniforms
            .push((name.to_owned(), value));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_world::{Properties, PropertyValue};

    fn object(id: u32, name: &str, x: i32, y: i32, kind: ObjectKind) -> MapObject {
        MapObject {
            id,
            name: name.into(),
            rect: TileRect::new(x, y, 1, 1),
            kind,
            properties: Properties::new(),
        }
    }

    fn town(objects: Vec<MapObject>) -> Map {
        Map {
            id: "town".into(),
            width: 6,
            height: 6,
            tile_width: 16,
            tile_height: 16,
            properties: Properties::new(),
            tilesets: vec![],
            layers: vec![],
            grid: LogicalGrid::new(6, 6),
            objects,
            encounters: vec![],
        }
    }

    fn keeper() -> MapObject {
        object(
            4,
            "keeper",
            2,
            2,
            ObjectKind::Npc {
                script: Some("maps/town.lua:keeper".into()),
                sprite: None,
                facing: Direction::Down,
            },
        )
    }

    fn map_with_npc() -> Map {
        town(vec![keeper()])
    }

    /// NPC at (2, 2), item at (2, 3) (uid `potion_ball`), sign at (3, 2), once trigger at
    /// (0, 0), repeating trigger at (1, 0).
    fn busy_town() -> Map {
        let mut item = object(
            5,
            "",
            2,
            3,
            ObjectKind::Item {
                item: "potion".into(),
                count: 2,
            },
        );
        item.properties
            .insert("uid", PropertyValue::String("potion_ball".into()));
        town(vec![
            keeper(),
            item,
            object(
                6,
                "board",
                3,
                2,
                ObjectKind::Sign {
                    script: None,
                    text: Some("text:board".into()),
                },
            ),
            object(
                7,
                "rival",
                0,
                0,
                ObjectKind::Trigger {
                    on_enter: Some("rival".into()),
                    once: true,
                },
            ),
            object(
                8,
                "",
                1,
                0,
                ObjectKind::Trigger {
                    on_enter: None,
                    once: false,
                },
            ),
        ])
    }

    fn standing(model: &mut WorldModel, pos: GridPos, facing: Direction) {
        model.player_pos = pos;
        model.player_facing = facing;
    }

    #[test]
    fn npcs_come_from_map_objects_and_walk_paths() {
        let mut model = WorldModel::default();
        model.enter_map(&map_with_npc(), GridPos::new(0, 0), Direction::Down);
        assert_eq!(model.npc("keeper").unwrap().object_id, 4);
        assert!(model.blocks(GridPos::new(2, 2)));
        let mut world = SharedWorld(Rc::new(RefCell::new(model)));
        world
            .npc_move("keeper", &[Direction::Up, Direction::Left])
            .unwrap();
        assert!(world.npc_move("keeper", &[Direction::Up]).is_err());
        assert!(world.npc_face("keeper", Direction::Up).is_ok());
        let rules = pkrs_world::TerrainRuleRegistry::new();
        let mut finished = Vec::new();
        for _ in 0..200 {
            finished.extend(world.get_mut().tick_npcs(&rules));
            if !finished.is_empty() {
                break;
            }
        }
        assert_eq!(finished, ["keeper"]);
        assert_eq!(
            world.npc_pos("keeper").unwrap(),
            (GridPos::new(1, 1), Direction::Left)
        );
        assert!(world.npc_pos("ghost").is_err());
    }

    #[test]
    fn requests_are_queued_for_the_overworld() {
        let mut world = SharedWorld::new();
        world.say("text:a", &TextArgs::new()).unwrap();
        world
            .choose(
                Some("q {n}"),
                &TextArgs::new().with("n", "1"),
                &["text:ui.yes".into(), "No".into()],
            )
            .unwrap();
        world.warp("house", GridPos::new(1, 2), None).unwrap();
        assert!(world.set_field_state("fly", true).is_err());
        assert!(world.set_field_state("surf", true).is_ok());
        assert!(world.set_field_state("mud:boots", true).is_ok());
        assert!(world.set_field_state("Mud:boots", true).is_err());
        let field = world.get().field_state();
        assert!(field.surf && field.is_active("mud:boots"));
        let w = world.get();
        assert_eq!(w.ui_requests.len(), 2);
        assert_eq!(
            w.ui_requests[1],
            UiRequest::Choose {
                prompt: Some("q 1".into()),
                options: vec!["Yes".into(), "No".into()],
            },
            "prompts take arguments, engine keys are localized"
        );
        assert_eq!(w.warp_request.as_ref().unwrap().map, "house");
    }

    #[test]
    fn language_switch_changes_later_texts() {
        let mut world = SharedWorld::new();
        world.get_mut().localizer = pkrs_data::Localizer::new("en", vec!["it".into()])
            .with_layer(crate::locale::engine_text());
        assert_eq!(world.language(), "en");
        assert_eq!(world.languages(), ["en", "it"]);
        assert!(world.set_language("fr").is_err());
        world.set_language("it").unwrap();
        assert_eq!(world.text("text:ui.yes", &TextArgs::new()), "Sì");
        world
            .choose(None, &TextArgs::new(), &["text:ui.yes".into()])
            .unwrap();
        world.notify("{0}!", &TextArgs::positional(["3"]));
        let w = world.get();
        assert_eq!(
            w.ui_requests[0],
            UiRequest::Choose {
                prompt: None,
                options: vec!["Sì".into()],
            }
        );
        assert_eq!(w.notifications, ["3!"]);
    }

    #[test]
    fn every_object_kind_spawns_an_entity() {
        let mut model = WorldModel::default();
        model.enter_map(&busy_town(), GridPos::new(0, 5), Direction::Up);
        assert_eq!(model.npcs.len(), 1);
        let ids: Vec<(&str, &str)> = model
            .entities
            .iter()
            .map(|e| (e.id.as_str(), e.key.as_str()))
            .collect();
        assert_eq!(
            ids,
            [
                ("potion_ball", "potion_ball"),
                ("board", "6"),
                ("rival", "7"),
                ("8", "8")
            ]
        );
        assert!(model.blocks(GridPos::new(2, 3)), "item balls are solid");
        assert!(
            !model.blocks(GridPos::new(3, 2)),
            "signs block via tiles only"
        );
    }

    #[test]
    fn interaction_target_is_the_faced_tile_npc_then_item_then_sign() {
        let mut model = WorldModel::default();
        model.enter_map(&busy_town(), GridPos::new(0, 5), Direction::Up);

        standing(&mut model, GridPos::new(2, 1), Direction::Down);
        assert_eq!(
            model.interaction_target(),
            InteractionTarget::Npc {
                id: "keeper".into(),
                script: Some("maps/town.lua:keeper".into())
            }
        );
        // Not facing it: nothing, even though the NPC is adjacent.
        standing(&mut model, GridPos::new(2, 1), Direction::Left);
        assert_eq!(model.interaction_target(), InteractionTarget::Tile);

        standing(&mut model, GridPos::new(2, 4), Direction::Up);
        assert_eq!(
            model.interaction_target(),
            InteractionTarget::Item {
                key: "potion_ball".into(),
                item: "potion".into(),
                count: 2
            }
        );

        standing(&mut model, GridPos::new(4, 2), Direction::Left);
        assert_eq!(
            model.interaction_target(),
            InteractionTarget::Sign {
                key: "6".into(),
                script: None,
                text: Some("text:board".into())
            }
        );

        // An NPC standing in front of a sign wins over the sign.
        model.npcs[0].walker = GridStepper::new(GridPos::new(3, 2), Direction::Down);
        assert!(matches!(
            model.interaction_target(),
            InteractionTarget::Npc { .. }
        ));
        // A hidden NPC is skipped.
        model.npcs[0].visible = false;
        assert!(matches!(
            model.interaction_target(),
            InteractionTarget::Sign { .. }
        ));
    }

    #[test]
    fn once_triggers_fire_once_and_repeating_ones_on_every_entry() {
        let mut model = WorldModel::default();
        let map = busy_town();
        model.enter_map(&map, GridPos::new(0, 1), Direction::Up);
        let enter = |m: &mut WorldModel, from, to| -> Vec<String> {
            m.enter_triggers(from, to)
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        };
        assert_eq!(
            enter(&mut model, GridPos::new(0, 1), GridPos::new(0, 0)),
            ["rival"]
        );
        assert!(model.state.map("town").unwrap().has_fired("7"));
        assert!(enter(&mut model, GridPos::new(0, 1), GridPos::new(0, 0)).is_empty());
        // Moving inside a trigger is not entering it.
        assert!(enter(&mut model, GridPos::new(1, 0), GridPos::new(1, 0)).is_empty());
        assert_eq!(
            enter(&mut model, GridPos::new(1, 1), GridPos::new(1, 0)),
            ["8"]
        );
        assert_eq!(
            enter(&mut model, GridPos::new(1, 1), GridPos::new(1, 0)),
            ["8"]
        );
        // Re-entering the map does not re-arm it.
        model.enter_map(&map, GridPos::new(0, 1), Direction::Up);
        assert!(enter(&mut model, GridPos::new(0, 1), GridPos::new(0, 0)).is_empty());
    }

    #[test]
    fn map_state_survives_leaving_and_re_entering() {
        let map = busy_town();
        let mut world = SharedWorld::new();
        world
            .get_mut()
            .enter_map(&map, GridPos::new(2, 4), Direction::Up);
        assert!(world.get_mut().pick_up_item("potion_ball"));
        assert!(!world.get_mut().pick_up_item("potion_ball"));
        assert_eq!(world.get().state.item_count("potion"), 2);
        assert_eq!(world.get().removed_entities, [5]);
        world.npc_set_visible("keeper", false).unwrap();
        world.set_map_var("visits", ScriptValue::Int(1));

        // Leave for another map: map vars are per map.
        world.get_mut().sync_map_state();
        let mut cave = town(vec![]);
        cave.id = "cave".into();
        world
            .get_mut()
            .enter_map(&cave, GridPos::new(0, 0), Direction::Up);
        assert_eq!(world.map_var("visits"), ScriptValue::Nil);
        world.get_mut().sync_map_state();

        world
            .get_mut()
            .enter_map(&map, GridPos::new(2, 4), Direction::Up);
        let w = world.get();
        assert!(w.item_at(GridPos::new(2, 3)).is_none(), "item stays gone");
        assert!(!w.npc("keeper").unwrap().visible, "npc stays hidden");
        assert!(w.removed_entities.is_empty());
        drop(w);
        assert_eq!(world.map_var("visits"), ScriptValue::Int(1));
    }

    #[test]
    fn moved_npcs_stay_moved_but_turned_ones_reset() {
        let map = map_with_npc();
        let mut model = WorldModel::default();
        model.enter_map(&map, GridPos::new(0, 0), Direction::Down);
        model.npcs[0].walker.face(Direction::Left);
        model.sync_map_state();
        assert!(model.state.map("town").unwrap().is_empty());
        model.enter_map(&map, GridPos::new(0, 0), Direction::Down);
        assert_eq!(model.npcs[0].walker.facing(), Direction::Down);

        model.npcs[0].walker = GridStepper::new(GridPos::new(4, 4), Direction::Right);
        model.sync_map_state();
        model.enter_map(&map, GridPos::new(0, 0), Direction::Down);
        let npc = &model.npcs[0];
        assert_eq!(
            (npc.walker.pos(), npc.walker.facing()),
            (GridPos::new(4, 4), Direction::Right)
        );
        assert_eq!(npc.home, GridPos::new(2, 2));
    }
}
