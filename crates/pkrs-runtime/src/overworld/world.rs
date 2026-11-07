//! The runtime's [`ScriptWorld`] adapter: overworld state scripts can read and change (§9.1).
//!
//! [`WorldModel`] holds flags, variables, the bag, map-local variables, the NPCs of the active
//! map and the requests scripts made this tick (dialogue, warp, camera). The script host gets
//! a [`SharedWorld`] handle to it; the overworld reads the requests back after the scripts and
//! events phases.

use std::cell::{Ref, RefCell, RefMut};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::rc::Rc;

use pkrs_core::is_action_name;
use pkrs_core::{Direction, GridPos};
use pkrs_script::{ApiError, ApiResult, CameraCommand, ScriptValue, ScriptWorld, TileInfo};
use pkrs_world::{
    FieldState, GridStepper, LogicalGrid, Map, MovementConfig, ObjectKind, PathProgress,
    ScriptedPath, TerrainRules,
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
    /// Tiled object id (scene entity id).
    pub object_id: u32,
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

/// Overworld state behind the [`ScriptWorld`] port.
#[derive(Debug)]
pub struct WorldModel {
    /// Global flags (§6.5).
    pub flags: BTreeMap<String, bool>,
    /// Global variables.
    pub vars: BTreeMap<String, ScriptValue>,
    /// Map-local variables, by map id.
    pub map_vars: BTreeMap<String, BTreeMap<String, ScriptValue>>,
    /// Items in the bag.
    pub bag: BTreeMap<String, u32>,
    /// Money.
    pub money: i64,
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
    /// NPCs of the active map.
    pub npcs: Vec<NpcState>,
    /// Dialogue requests, oldest first.
    pub ui_requests: VecDeque<UiRequest>,
    /// `ui.notify` messages not yet shown.
    pub notifications: Vec<String>,
    /// Pending `map.warp`.
    pub warp_request: Option<WarpRequest>,
    /// Camera commands, in call order.
    pub camera: Vec<CameraCommand>,
    /// `once` triggers that already fired, as `(map, key)` (§6.5).
    pub fired_triggers: BTreeSet<(String, String)>,
}

impl Default for WorldModel {
    fn default() -> Self {
        Self {
            flags: BTreeMap::new(),
            vars: BTreeMap::new(),
            map_vars: BTreeMap::new(),
            bag: BTreeMap::new(),
            money: 0,
            map_id: None,
            grid: None,
            player_pos: GridPos::new(0, 0),
            player_facing: Direction::Down,
            player_face_request: None,
            script_lock: false,
            field_states: BTreeMap::new(),
            npcs: Vec::new(),
            ui_requests: VecDeque::new(),
            notifications: Vec::new(),
            warp_request: None,
            camera: Vec::new(),
            fired_triggers: BTreeSet::new(),
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

impl WorldModel {
    /// Replaces the map-dependent state with `map`'s NPCs and grid, the player standing on
    /// `player` facing `facing`.
    pub fn enter_map(&mut self, map: &Map, player: GridPos, facing: Direction) {
        self.map_id = Some(map.id.clone());
        self.grid = Some(map.grid.clone());
        self.player_pos = player;
        self.player_facing = facing;
        self.warp_request = None;
        let config = MovementConfig {
            tap_turn_ticks: 1,
            ..MovementConfig::default()
        };
        self.npcs = map
            .objects
            .iter()
            .filter_map(|o| match &o.kind {
                ObjectKind::Npc {
                    script,
                    sprite,
                    facing,
                } => Some(NpcState {
                    id: if o.name.is_empty() {
                        o.id.to_string()
                    } else {
                        o.name.clone()
                    },
                    object_id: o.id,
                    walker: GridStepper::with_config(o.pos(), *facing, config),
                    path: None,
                    sprite: sprite.clone(),
                    visible: true,
                    script: script.clone(),
                }),
                _ => None,
            })
            .collect();
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

    /// `true` when a visible NPC stands on `pos` (player occupancy).
    #[must_use]
    pub fn blocks(&self, pos: GridPos) -> bool {
        self.npc_at(pos).is_some()
    }

    /// Advances every NPC on a scripted path by one tick; returns the ids whose path ended.
    pub fn tick_npcs(&mut self, rules: &dyn TerrainRules) -> Vec<String> {
        let Some(grid) = self.grid.as_ref() else {
            return Vec::new();
        };
        let player = self.player_pos;
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
        self.get().flags.get(name).copied().unwrap_or(false)
    }

    fn set_flag(&mut self, name: &str, value: bool) {
        self.get_mut().flags.insert(name.to_owned(), value);
    }

    fn var(&self, name: &str) -> ScriptValue {
        self.get()
            .vars
            .get(name)
            .cloned()
            .unwrap_or(ScriptValue::Nil)
    }

    fn set_var(&mut self, name: &str, value: ScriptValue) {
        let mut w = self.get_mut();
        if value.is_nil() {
            w.vars.remove(name);
        } else {
            w.vars.insert(name.to_owned(), value);
        }
    }

    fn give_item(&mut self, item: &str, count: u32) -> ApiResult<()> {
        let mut w = self.get_mut();
        let slot = w.bag.entry(item.to_owned()).or_default();
        *slot = slot.saturating_add(count);
        tracing::info!(target: "script", item, count, "give_item");
        Ok(())
    }

    fn bag(&self) -> Vec<(String, u32)> {
        self.get()
            .bag
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    fn money(&self) -> i64 {
        self.get().money
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
        w.map_vars
            .get(w.map_id.as_deref().unwrap_or_default())
            .and_then(|vars| vars.get(name))
            .cloned()
            .unwrap_or(ScriptValue::Nil)
    }

    fn set_map_var(&mut self, name: &str, value: ScriptValue) {
        let mut w = self.get_mut();
        let map = w.map_id.clone().unwrap_or_default();
        let vars = w.map_vars.entry(map).or_default();
        if value.is_nil() {
            vars.remove(name);
        } else {
            vars.insert(name.to_owned(), value);
        }
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

    fn say(&mut self, text: &str) -> ApiResult<()> {
        self.get_mut()
            .ui_requests
            .push_back(UiRequest::Say(text.to_owned()));
        Ok(())
    }

    fn choose(&mut self, prompt: Option<&str>, options: &[String]) -> ApiResult<()> {
        self.get_mut().ui_requests.push_back(UiRequest::Choose {
            prompt: prompt.map(str::to_owned),
            options: options.to_vec(),
        });
        Ok(())
    }

    fn notify(&mut self, text: &str) {
        tracing::info!(target: "script", text, "notify");
        self.get_mut().notifications.push(text.to_owned());
    }

    fn camera(&mut self, command: CameraCommand) -> ApiResult<()> {
        self.get_mut().camera.push(command);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_world::{MapObject, Properties, TileRect};

    fn map_with_npc() -> Map {
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
            objects: vec![MapObject {
                id: 4,
                name: "keeper".into(),
                rect: TileRect::new(2, 2, 1, 1),
                kind: ObjectKind::Npc {
                    script: Some("maps/town.lua:keeper".into()),
                    sprite: None,
                    facing: Direction::Down,
                },
                properties: Properties::new(),
            }],
            encounters: vec![],
        }
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
        world.say("text:a").unwrap();
        world
            .choose(Some("q"), &["Yes".into(), "No".into()])
            .unwrap();
        world.warp("house", GridPos::new(1, 2), None).unwrap();
        world.set_map_var("x", ScriptValue::Int(1));
        assert_eq!(world.map_var("x"), ScriptValue::Int(1));
        assert!(world.set_field_state("fly", true).is_err());
        assert!(world.set_field_state("surf", true).is_ok());
        assert!(world.set_field_state("mud:boots", true).is_ok());
        assert!(world.set_field_state("Mud:boots", true).is_err());
        let field = world.get().field_state();
        assert!(field.surf && field.is_active("mud:boots"));
        let w = world.get();
        assert_eq!(w.ui_requests.len(), 2);
        assert_eq!(w.warp_request.as_ref().unwrap().map, "house");
    }
}
