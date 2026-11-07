//! Overworld scripting (§6.4, §9.1–§9.3): the glue between the map, the player and Lua.
//!
//! [`OverworldScripts`] owns the [`LuaScriptHost`], its [`Scheduler`] and the [`SharedWorld`]
//! the API v0 bindings talk to. The overworld calls it at fixed points of the tick (§3.3):
//!
//! 1. [`run_scripts_phase`](OverworldScripts::run_scripts_phase): apply the wake signals
//!    gathered last tick (dialogue closed, path finished) and step the scheduler.
//! 2. During the overworld update: [`on_player_step`](OverworldScripts::on_player_step)
//!    (`step`, `trigger_enter`), [`interact`](OverworldScripts::interact) (`interact` plus
//!    the object's own script) and [`tick_npcs`](OverworldScripts::tick_npcs) (NPC paths).
//! 3. [`flush_events`](OverworldScripts::flush_events): start every queued event's listeners.
//!
//! Map changes go through [`exit_map`](OverworldScripts::exit_map) (dispatch `map_exit`, then
//! unload the map script's handlers) and [`enter_map`](OverworldScripts::enter_map) (load
//! `scripts/maps/<id>.lua` into its own environment, queue `map_enter`).

mod overlay;
mod world;

pub use overlay::{CHARS_PER_TICK, ScreenFade, ScriptOverlay};
pub use world::{FIELD_STATES, NpcState, SharedWorld, UiRequest, WarpRequest, WorldModel};

use std::collections::BTreeSet;

use pkrs_core::{Direction, GridPos};
use pkrs_script::{
    CameraCommand, DispatchReport, LuaScriptHost, Scheduler, ScriptEvent, ScriptHost, ScriptOwner,
    ScriptValue, TaskKind, WakeSignal,
};
use pkrs_world::{Map, ObjectKind, TerrainRuleRegistry};

/// Game script run once when the game starts (if the project has one).
pub const MAIN_SCRIPT: &str = "scripts/main.lua";

/// The per-map script of `map_id` (§6.4).
#[must_use]
pub fn map_script_path(map_id: &str) -> String {
    format!("scripts/maps/{map_id}.lua")
}

/// Lua host, scheduler and world for the overworld.
pub struct OverworldScripts {
    host: LuaScriptHost,
    scheduler: Scheduler,
    world: SharedWorld,
    wakes: Vec<WakeSignal>,
    game_files: BTreeSet<String>,
    map_owner: Option<ScriptOwner>,
}

impl OverworldScripts {
    /// Takes over `host` (plugins already loaded) and attaches a fresh world to it.
    #[must_use]
    pub fn new(mut host: LuaScriptHost) -> Self {
        let world = SharedWorld::new();
        host.set_world(Box::new(world.clone()));
        Self {
            host,
            scheduler: Scheduler::new(),
            world,
            wakes: Vec::new(),
            game_files: BTreeSet::new(),
            map_owner: None,
        }
    }

    /// The script host.
    pub fn host_mut(&mut self) -> &mut LuaScriptHost {
        &mut self.host
    }

    /// The scheduler.
    #[must_use]
    pub fn scheduler(&self) -> &Scheduler {
        &self.scheduler
    }

    /// The world scripts see.
    #[must_use]
    pub fn world(&self) -> &SharedWorld {
        &self.world
    }

    /// Owner of the active map's script, if a map is loaded.
    #[must_use]
    pub fn map_owner(&self) -> Option<&ScriptOwner> {
        self.map_owner.as_ref()
    }

    /// Starts the game on `map`: runs [`MAIN_SCRIPT`], queues `game_start`, enters the map.
    pub fn start_game(&mut self, map: &Map, player: GridPos, facing: Direction) {
        self.load_game_file(MAIN_SCRIPT);
        self.host.emit_event(ScriptEvent::game_start());
        self.enter_map(map, player, facing);
    }

    /// Loads `map`'s NPCs into the world, runs its script in a fresh [`ScriptOwner::Map`]
    /// environment and queues `map_enter`.
    pub fn enter_map(&mut self, map: &Map, player: GridPos, facing: Direction) {
        self.world.get_mut().enter_map(map, player, facing);
        let owner = ScriptOwner::Map(map.id.clone());
        let path = map_script_path(&map.id);
        if self.host.script_exists(&path) {
            if let Err(err) = self.host.run_file(&owner, &path) {
                tracing::error!(target: "script", map = %map.id, "map script failed: {err}");
            }
        }
        self.map_owner = Some(owner);
        self.host.emit_event(ScriptEvent::map_enter(&map.id));
    }

    /// Leaves the active map: dispatches `map_exit` while its handlers are still registered,
    /// ends NPC paths (waking their tasks) and unloads the map script.
    pub fn exit_map(&mut self) {
        let Some(map) = self.world.get().map_id.clone() else {
            return;
        };
        self.host.emit_event(ScriptEvent::map_exit(&map));
        self.flush_events();
        let walking: Vec<String> = self
            .world
            .get()
            .npcs
            .iter()
            .filter(|n| n.path.is_some())
            .map(|n| n.id.clone())
            .collect();
        for npc in walking {
            self.wakes.push(WakeSignal::PathFinished { npc });
        }
        if let Some(owner) = self.map_owner.take() {
            let removed = self.host.unload_owner(&owner);
            tracing::debug!(target: "script", %owner, removed, "map script unloaded");
        }
    }

    /// §3.3 step 2: applies last tick's wake signals and resumes ready tasks.
    pub fn run_scripts_phase(&mut self) {
        for signal in self.wakes.drain(..) {
            self.scheduler.signal(signal);
        }
        self.scheduler.step(&mut self.host);
    }

    /// §3.3 step 5: starts the listeners of every queued event, in emission order.
    pub fn flush_events(&mut self) -> DispatchReport {
        self.scheduler.dispatch_events(&mut self.host)
    }

    /// `true` while an interact/trigger task runs or a script called `player.lock()`.
    #[must_use]
    pub fn player_locked(&self) -> bool {
        self.scheduler.player_locked() || self.world.get().script_lock
    }

    /// Filters the player's movement request through the script locks (§9.3).
    #[must_use]
    pub fn gate(&self, dir: Option<Direction>) -> Option<Direction> {
        if self.world.get().script_lock {
            None
        } else {
            self.scheduler.player_lock().gate(dir)
        }
    }

    /// Tells scripts where the player is.
    pub fn sync_player(&mut self, pos: GridPos, facing: Direction) {
        let mut w = self.world.get_mut();
        w.player_pos = pos;
        w.player_facing = facing;
    }

    /// The player completed a step from `from` to `pos`: queues `step`, then `trigger_enter`
    /// for every trigger zone entered (and starts its `on_enter` script).
    pub fn on_player_step(&mut self, map: &Map, from: GridPos, pos: GridPos, facing: Direction) {
        self.sync_player(pos, facing);
        self.host.emit_event(ScriptEvent::step(pos, facing));
        for object in &map.objects {
            let ObjectKind::Trigger { on_enter, once } = &object.kind else {
                continue;
            };
            if !object.rect.contains(pos) || object.rect.contains(from) {
                continue;
            }
            let key = (map.id.clone(), object.persistent_key());
            if *once && !self.world.get_mut().fired_triggers.insert(key) {
                continue;
            }
            let id = if object.name.is_empty() {
                object.persistent_key()
            } else {
                object.name.clone()
            };
            let event = ScriptEvent::trigger_enter(&id, pos);
            let payload = event.payload.clone();
            self.host.emit_event(event);
            if let Some(script) = on_enter {
                self.spawn_script(script, &[payload], TaskKind::Trigger);
            }
        }
    }

    /// The player pressed Confirm: talks to the NPC or reads the sign in front of them (or
    /// reports the tile). Queues `interact` and starts the object's script.
    pub fn interact(&mut self, map: &Map) {
        let (target, npc) = {
            let w = self.world.get();
            let target = w.player_pos.neighbour(w.player_facing);
            (
                target,
                w.npc_at(target).map(|n| (n.id.clone(), n.script.clone())),
            )
        };
        if let Some((id, script)) = npc {
            self.host.emit_event(ScriptEvent::interact_npc(&id, target));
            if let Some(script) = script {
                self.spawn_script(&script, &[ScriptValue::Npc(id)], TaskKind::Interact);
            }
            return;
        }
        let sign = map.objects.iter().find_map(|o| match &o.kind {
            ObjectKind::Sign { script, text } if o.rect.contains(target) => {
                Some((o.persistent_key(), script.clone(), text.clone()))
            }
            _ => None,
        });
        match sign {
            Some((id, script, text)) => {
                self.host
                    .emit_event(ScriptEvent::interact_sign(&id, text.as_deref(), target));
                match (script, text) {
                    (Some(script), _) => self.spawn_script(&script, &[], TaskKind::Interact),
                    (None, Some(text)) => {
                        self.world
                            .get_mut()
                            .ui_requests
                            .push_back(UiRequest::Say(text));
                    }
                    (None, None) => {}
                }
            }
            None => self.host.emit_event(ScriptEvent::interact_tile(target)),
        }
    }

    /// Advances NPC paths; finished paths wake their `npc:move` next tick.
    pub fn tick_npcs(&mut self, rules: &TerrainRuleRegistry) {
        let finished = self.world.get_mut().tick_npcs(rules);
        self.wakes.extend(
            finished
                .into_iter()
                .map(|npc| WakeSignal::PathFinished { npc }),
        );
    }

    /// The open dialogue closed with `answer`; its task resumes next tick.
    pub fn close_dialogue(&mut self, answer: ScriptValue) {
        self.wakes.push(WakeSignal::DialogueClosed(answer));
    }

    /// Next dialogue to show.
    pub fn take_ui_request(&mut self) -> Option<UiRequest> {
        self.world.get_mut().ui_requests.pop_front()
    }

    /// Pending `map.warp`.
    pub fn take_warp_request(&mut self) -> Option<WarpRequest> {
        self.world.get_mut().warp_request.take()
    }

    /// Camera commands issued since the last call.
    pub fn take_camera_commands(&mut self) -> Vec<CameraCommand> {
        std::mem::take(&mut self.world.get_mut().camera)
    }

    /// Facing requested with `player.face`.
    pub fn take_player_face(&mut self) -> Option<Direction> {
        self.world.get_mut().player_face_request.take()
    }

    fn load_game_file(&mut self, path: &str) -> bool {
        if self.game_files.contains(path) {
            return true;
        }
        if !self.host.script_exists(path) {
            return false;
        }
        self.game_files.insert(path.to_owned());
        match self.host.run_file(&ScriptOwner::Game, path) {
            Ok(_) => true,
            Err(err) => {
                tracing::error!(target: "script", path, "script failed to load: {err}");
                false
            }
        }
    }

    /// Starts the function an object property names (§6.2): `"maps/route1.lua:old_man"`
    /// (file under `scripts/`, then function) or a bare `"old_man"` (the map script's
    /// function, else the game's).
    fn spawn_script(&mut self, reference: &str, args: &[ScriptValue], kind: TaskKind) {
        let (owner, function) = match reference.rsplit_once(':') {
            Some((file, function)) => {
                let path = format!("scripts/{}", file.trim_start_matches('/'));
                let is_map_script = self
                    .map_owner
                    .as_ref()
                    .and_then(|o| match o {
                        ScriptOwner::Map(id) => Some(map_script_path(id)),
                        _ => None,
                    })
                    .is_some_and(|p| p == path);
                if is_map_script {
                    (self.map_owner.clone(), function)
                } else if self.load_game_file(&path) {
                    (Some(ScriptOwner::Game), function)
                } else {
                    tracing::error!(target: "script", reference, "script file `{path}` not found");
                    return;
                }
            }
            None if !reference.contains('/') => {
                let owner = match &self.map_owner {
                    Some(map) if self.host.has_function(map, reference) => map.clone(),
                    _ => ScriptOwner::Game,
                };
                (Some(owner), reference)
            }
            None => {
                tracing::error!(
                    target: "script",
                    reference,
                    "script references must be `file.lua:function` or `function`"
                );
                return;
            }
        };
        let Some(owner) = owner else {
            return;
        };
        // Errors are logged by the scheduler; the player is never left locked.
        let _ = self
            .scheduler
            .spawn(&mut self.host, &owner, function, args, kind);
    }
}
