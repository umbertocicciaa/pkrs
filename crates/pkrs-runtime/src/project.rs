//! Running a game project from loose files (`--project <dir>`, used by `pkrs run`, §2.3, §14.1).
//!
//! Loads `pkrs.toml`, mounts the project folder through the assets VFS, imports the start map
//! with the Tiled adapter and draws it with the player at the start position. Dev saves go to
//! `<project>/saves/` (§12).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pkrs_assets::{FolderVfs, Vfs, VfsPath};
use pkrs_battle::{BattleResult, NullBattleHooks, RngPolicy};
use pkrs_core::{Direction, GridPos, Pcg64Rng, ProjectManifest, Version};
use pkrs_data::{GameData, LoadOptions, Localizer, UnknownLanguage};
use pkrs_platform::{
    Action, BuiltinAction, DisplaySettings, Input, InputFacadeConfig, Key, Platform, UserPaths,
};
use pkrs_render::{CameraTarget, Rect, Renderer, TextureHandle};
use pkrs_script::{
    BattleRequest, CameraCommand, CameraTarget as ScriptCameraTarget, LuaScriptHost, ScriptValue,
    TaskId, battle_result_value,
};
use pkrs_world::{
    GridStepper, LinkCrossing, Map, MapImporter, MovementConfig, StepEvent, TerrainQuery,
    TerrainRuleRegistry, TerrainRules, TerrainStepVerdict, TiledImporter,
    Transition as WarpTransition, WORLD_RON, Warp, WorldLayout, neighbor_pixel_offset,
};

use crate::battle::{BattleContent, BattleScreen, PostBattle};
use crate::data::load_game_data;
use crate::default_theme::build_default_ui_theme;
use crate::encounters::{Encounters, wild_battle_request};
use crate::main_loop::SimulationHost;
use crate::materials::MaterialCache;
use crate::overworld::{EntityKind, OverworldScripts, ScreenFade, ScriptOverlay};
use crate::party::PlayerParty;
use crate::scene::{MapScene, NeighborBackdrop, PLAYER_ENTITY, tile_sheet};
use crate::ui_theme::UiTheme;

mod save;

pub use save::{GameSaveError, game_migration, save_store};

/// Scripting API levels this runtime supports (§9.7).
pub const SUPPORTED_API: std::ops::RangeInclusive<u32> = 1..=1;

/// Sprite used for the player when the project ships one (`sprites/player.png`).
pub const PLAYER_SPRITE: &str = "sprites/player.png";

/// Why a project could not be started.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    /// `pkrs.toml` is missing or invalid.
    #[error(transparent)]
    Manifest(#[from] pkrs_core::ManifestError),
    /// Engine version or id parsing failed.
    #[error(transparent)]
    Core(#[from] pkrs_core::CoreError),
    /// The project folder could not be mounted, or a file could not be read.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// The start map failed to load.
    #[error(transparent)]
    Map(#[from] pkrs_world::MapError),
    /// World layout (`world.ron`) failed to load or validate.
    #[error(transparent)]
    World(#[from] pkrs_world::WorldError),
    /// A texture could not be decoded or uploaded.
    #[error(transparent)]
    Render(#[from] pkrs_render::RenderError),
    /// The scene could not be built.
    #[error(transparent)]
    Scene(#[from] crate::scene::SceneError),
    /// The map has no tileset image for a tileset.
    #[error("tileset `{0}` has no image (only single-image tilesets are supported)")]
    TilesetImage(String),
    /// The start or warp position is outside the map.
    #[error("start position {pos} is outside map `{map}` ({width}x{height})")]
    StartOutOfBounds {
        /// Map id.
        map: String,
        /// Requested tile.
        pos: GridPos,
        /// Map width.
        width: u32,
        /// Map height.
        height: u32,
    },
}

/// Overrides from `pkrs run --map <id> --warp x,y`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProjectOptions {
    /// Project folder (contains `pkrs.toml`).
    pub dir: PathBuf,
    /// Start on this map instead of `start.map`.
    pub map: Option<String>,
    /// Start on this tile instead of `start.position` (or the map's `default` spawn).
    pub warp: Option<GridPos>,
}

/// Dev [`UserPaths`] for `pkrs run`: everything under `<project>/saves/` (§12).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DevPaths {
    data: PathBuf,
    config: PathBuf,
    logs: PathBuf,
}

impl DevPaths {
    /// `<project>/saves`, `<project>/saves/config`, `<project>/saves/logs`.
    #[must_use]
    pub fn for_project(dir: &Path) -> Self {
        let saves = dir.join("saves");
        Self {
            config: saves.join("config"),
            logs: saves.join("logs"),
            data: saves,
        }
    }
}

impl UserPaths for DevPaths {
    fn data_dir(&self) -> &Path {
        &self.data
    }

    fn config_dir(&self) -> &Path {
        &self.config
    }

    fn logs_dir(&self) -> &Path {
        &self.logs
    }
}

/// A validated project and where play starts.
#[derive(Clone)]
pub struct Project {
    /// The manifest.
    pub manifest: ProjectManifest,
    /// The project folder, mounted read-only.
    pub vfs: FolderVfs,
    /// Start map id.
    pub map_id: String,
    /// Explicit start tile (`None`: decided once the map is loaded).
    pub warp: Option<GridPos>,
    /// Dev saves location.
    pub paths: DevPaths,
}

impl Project {
    /// Loads and validates `<dir>/pkrs.toml` against this runtime and mounts the folder.
    pub fn open(options: &ProjectOptions) -> Result<Self, ProjectError> {
        let manifest = ProjectManifest::load(&options.dir)?;
        let engine = Version::parse(env!("CARGO_PKG_VERSION"))?;
        manifest.check_engine(&engine, SUPPORTED_API)?;
        let vfs = FolderVfs::new(&options.dir)?;
        Ok(Self {
            map_id: options
                .map
                .clone()
                .unwrap_or_else(|| manifest.start.map.clone()),
            warp: options.warp,
            paths: DevPaths::for_project(&options.dir),
            manifest,
            vfs,
        })
    }

    /// Display settings from `[display]`.
    #[must_use]
    pub fn display(&self) -> DisplaySettings {
        let d = &self.manifest.display;
        DisplaySettings {
            width: d.width,
            height: d.height,
            scale: d.scale,
            fullscreen: d.fullscreen,
        }
    }

    /// Imports `maps/<map_id>.tmx`.
    pub fn load_map(&self) -> Result<Map, ProjectError> {
        self.load_map_by_id(&self.map_id)
    }

    /// Imports `maps/<map_id>.tmx` by stem (§6.4 warps).
    pub fn load_map_by_id(&self, map_id: &str) -> Result<Map, ProjectError> {
        let path = VfsPath::new(&format!("maps/{map_id}.tmx"))?;
        Ok(TiledImporter::new().import(&self.vfs, &path)?)
    }

    /// Where the player starts on `map`: `--warp`, else `start.position` on the start map,
    /// else the map's `default` spawn, else the map centre.
    #[must_use]
    pub fn start_tile(&self, map: &Map) -> (GridPos, Direction) {
        let facing = self.manifest.start.facing;
        if let Some(pos) = self.warp {
            return (pos, facing);
        }
        if self.map_id == self.manifest.start.map {
            return (self.manifest.start.position, facing);
        }
        if let Some(spawn) = map.spawn("default") {
            let facing = match &spawn.kind {
                pkrs_world::ObjectKind::Spawn { facing: Some(f) } => *f,
                _ => facing,
            };
            return (spawn.pos(), facing);
        }
        let centre = GridPos::new((map.width / 2) as i32, (map.height / 2) as i32);
        (centre, facing)
    }
}

/// Ticks per grid step when walking (§6.3 default walking speed).
pub const STEP_TICKS: u32 = MovementConfig::WALK_STEP_TICKS;

/// Half-transition length for [`WarpTransition::Fade`] at 60 Hz (§6.4).
const WARP_FADE_TICKS: u32 = 20;

/// Pending destination after the outgoing half of a warp (§6.4).
#[derive(Clone, Debug)]
struct PendingWarp {
    to_map: String,
    to: GridPos,
    facing: Option<Direction>,
    transition: WarpTransition,
}

/// Whether the player is in the middle of a warp transition.
#[derive(Debug, Default)]
enum WarpPhase {
    /// Normal overworld play.
    #[default]
    None,
    /// Fade out (or door buildup) before swapping maps.
    OutFade {
        remaining: u32,
        pending: Box<PendingWarp>,
    },
    /// Fade in after the map swap.
    InFade { remaining: u32 },
}

fn transition_ticks_out(transition: WarpTransition) -> u32 {
    match transition {
        WarpTransition::None => 0,
        WarpTransition::Fade => WARP_FADE_TICKS,
        WarpTransition::Door => WARP_FADE_TICKS * 2,
    }
}

fn transition_ticks_in(transition: WarpTransition) -> u32 {
    match transition {
        WarpTransition::None => 0,
        WarpTransition::Fade => WARP_FADE_TICKS,
        WarpTransition::Door => WARP_FADE_TICKS * 2,
    }
}

/// Tile sizes for neighbours and the `LinkCrossing` helper (§6.4).
struct CrossingContext {
    layout: WorldLayout,
    map_id: String,
    map_size: (u32, u32),
    neighbor_sizes: BTreeMap<String, (u32, u32)>,
}

impl CrossingContext {
    fn link_crossing(&self) -> LinkCrossing<'_> {
        LinkCrossing {
            layout: &self.layout,
            map_id: &self.map_id,
            map_size: self.map_size,
            neighbor_sizes: &self.neighbor_sizes,
        }
    }
}

/// Cached neighbour map ready for edge-strip drawing.
struct NeighborCache {
    dir: Direction,
    map: Map,
    textures: Vec<TextureHandle>,
    backdrop: NeighborBackdrop,
}

fn neighbor_cache(
    dir: Direction,
    map: Map,
    textures: Vec<TextureHandle>,
    offset_px: (f32, f32),
) -> NeighborCache {
    let sheets: Vec<_> = map
        .tilesets
        .iter()
        .zip(&textures)
        .map(|(ts, &tex)| tile_sheet(ts, tex))
        .collect();
    let backdrop = NeighborBackdrop {
        map: map.clone(),
        sheets,
        offset_px,
    };
    NeighborCache {
        dir,
        map,
        textures,
        backdrop,
    }
}

/// The player's terrain rules (§6.3): Rust rules registered on the host first, then plugin
/// rules (Lua `world.register_terrain`, data-defined terrains) from the script host.
struct PlayerTerrain<'a> {
    builtin: &'a TerrainRuleRegistry,
    scripts: Option<&'a LuaScriptHost>,
}

impl TerrainRules for PlayerTerrain<'_> {
    fn verdict(&self, query: &TerrainQuery<'_>) -> TerrainStepVerdict {
        if self.builtin.get(query.terrain).is_some() {
            return self.builtin.verdict(query);
        }
        match self.scripts {
            Some(scripts) => scripts.verdict(query),
            None => TerrainStepVerdict::Allow,
        }
    }
}

/// The overworld of a loaded project: map scene, player, NPCs, scripts and input.
pub struct ProjectHost {
    project: Project,
    renderer: Renderer,
    scene: MapScene,
    input: Input,
    terrain_rules: TerrainRuleRegistry,
    /// Custom input actions registered by scripts (`input.register_action`, §10).
    custom_actions: Vec<(String, Action)>,
    walker: GridStepper,
    player_sprite: (TextureHandle, Rect),
    npc_sprite: (TextureHandle, Rect),
    /// `sprites/<id>.png` per NPC `sprite` id (the marker when it is missing or invalid).
    npc_sprites: BTreeMap<String, (TextureHandle, Rect)>,
    item_sprite: (TextureHandle, Rect),
    ticks: u64,
    warp_phase: WarpPhase,
    scripts: Option<OverworldScripts>,
    ui_theme: Option<UiTheme>,
    overlay: Option<ScriptOverlay>,
    fade: ScreenFade,
    /// Tile the player last stood on between steps (trigger zones fire on entry).
    player_tile: GridPos,
    crossing: Option<CrossingContext>,
    neighbors: Vec<NeighborCache>,
    active_textures: Vec<TextureHandle>,
    /// The running battle and the script task waiting on it (§7.2); no task for a wild
    /// encounter started by a step.
    battle: Option<(Option<TaskId>, BattleScreen)>,
    /// The project's content database (§8.1, merged with the plugins' content), loaded on
    /// the first battle or encounter.
    game_data: Option<GameData>,
    /// Battle data taken from `game_data`, on the first battle.
    battle_content: Option<BattleContent>,
    /// Stream battle RNG policies derive from (`RngPolicy::Local`, §7.2).
    battle_rng: Pcg64Rng,
    /// Battle messages shown by finished battles.
    battle_texts: u64,
    /// The player's party for the session, built from `data/trainers/player.ron` on the
    /// first battle (§7.3, until saves own it).
    party: Option<PlayerParty>,
    /// After a won battle: exp, level ups, moves and evolutions, with the battle's result
    /// (the finished screen stays in `battle` behind it).
    post_battle: Option<(BattleResult, PostBattle)>,
    /// Wild encounter tables and roll stream (§6.3).
    encounters: Encounters,
    /// Play time and plugin data carried over from a loaded save (§12).
    save_session: save::SaveSession,
    /// Active language and text tables (§5.5); the script world holds the live copy when
    /// scripting is on ([`localizer`](Self::localizer)).
    localizer: Localizer,
    /// Materials referenced by maps and NPCs, compiled once per id (§5.6).
    materials: MaterialCache,
}

/// Seed of the stream battles derive their RNG from until the save owns the game RNG (§12).
pub const DEFAULT_BATTLE_SEED: u64 = 0x0062_6174_746c_6573;

impl ProjectHost {
    /// Loads the start map of `project` and uploads its textures through `renderer`. No
    /// scripts run; see [`with_scripts`](Self::with_scripts).
    pub fn new(project: &Project, renderer: Renderer) -> Result<Self, ProjectError> {
        Self::build(project, renderer, None)
    }

    /// Like [`new`](Self::new), with the overworld driven by `scripts` (plugins already
    /// loaded): `scripts/main.lua` runs, `game_start` and `map_enter` fire, the start map's
    /// script is loaded and NPCs, triggers and signs run their scripts (§6.4, §9.1–§9.3).
    pub fn with_scripts(
        project: &Project,
        renderer: Renderer,
        scripts: LuaScriptHost,
    ) -> Result<Self, ProjectError> {
        Self::build(project, renderer, Some(scripts))
    }

    fn build(
        project: &Project,
        mut renderer: Renderer,
        scripts: Option<LuaScriptHost>,
    ) -> Result<Self, ProjectError> {
        let map = project.load_map()?;
        let textures = upload_map_textures(project, &mut renderer, &map)?;
        let (start, facing) = project.start_tile(&map);
        if !map.grid.in_bounds(start) {
            return Err(ProjectError::StartOutOfBounds {
                map: map.id.clone(),
                pos: start,
                width: map.width,
                height: map.height,
            });
        }
        let (tw, th) = (map.tile_width, map.tile_height);
        let player_sprite = load_player_sprite(project, &mut renderer, tw, th)?;
        tracing::info!(map = %map.id, start = %start, "map_enter");

        let mut scene = MapScene::new(map, &textures)?;
        let mut materials = MaterialCache::new();
        scene.set_materials(materials.for_map(scene.map(), &project.vfs, &mut renderer));
        scene.place_entity(
            PLAYER_ENTITY,
            start,
            player_sprite.0,
            player_sprite.1,
            false,
        );
        let input = Input::from_manifest(InputFacadeConfig::default(), &project.manifest.input)
            .unwrap_or_else(|_| Input::new(InputFacadeConfig::default()));
        let npc_sprite = marker_sprite(&mut renderer, tw, th, [60, 90, 230, 255])?;
        let item_sprite = marker_sprite(&mut renderer, tw, th, [240, 200, 40, 255])?;
        let ui_theme = match &scripts {
            Some(_) => match build_default_ui_theme(&mut renderer) {
                Ok(theme) => Some(theme),
                Err(err) => {
                    tracing::warn!(%err, "no UI theme: script dialogue closes immediately");
                    None
                }
            },
            None => None,
        };
        let mut host = Self {
            project: project.clone(),
            renderer,
            scene,
            input,
            terrain_rules: TerrainRuleRegistry::new(),
            custom_actions: Vec::new(),
            walker: GridStepper::new(start, facing),
            player_sprite,
            npc_sprite,
            npc_sprites: BTreeMap::new(),
            item_sprite,
            ticks: 0,
            warp_phase: WarpPhase::None,
            scripts: scripts.map(OverworldScripts::new),
            ui_theme,
            overlay: None,
            fade: ScreenFade::default(),
            player_tile: start,
            crossing: None,
            neighbors: Vec::new(),
            active_textures: textures,
            battle: None,
            game_data: None,
            battle_content: None,
            battle_rng: Pcg64Rng::from_seed(DEFAULT_BATTLE_SEED),
            battle_texts: 0,
            party: None,
            post_battle: None,
            encounters: Encounters::default(),
            save_session: save::SaveSession::default(),
            localizer: crate::locale::project_localizer(
                &project.vfs,
                &project.manifest.locale,
                &text_plugins(project),
            ),
            materials,
        };
        if let Some(scripts) = host.scripts.as_ref() {
            scripts.world().get_mut().localizer = host.localizer.clone();
        }
        host.bootstrap_world_layout()?;
        if let Some(scripts) = host.scripts.as_mut() {
            scripts.start_game(host.scene.map(), start, facing);
            host.place_npcs();
        }
        if let Some(scripts) = host.scripts.as_mut() {
            scripts.flush_events();
        }
        host.apply_script_requests();
        Ok(host)
    }

    /// The running battle, if any.
    #[must_use]
    pub fn battle(&self) -> Option<&BattleScreen> {
        self.battle.as_ref().map(|(_, screen)| screen)
    }

    /// Battle messages shown so far, over all battles (post-battle texts included).
    #[must_use]
    pub fn battle_texts_shown(&self) -> u64 {
        self.battle_texts
            .saturating_add(self.battle().map_or(0, BattleScreen::texts_shown))
            .saturating_add(self.post_battle().map_or(0, PostBattle::texts_shown))
    }

    /// The post-battle flow (exp, level ups, moves, evolutions) while it runs.
    #[must_use]
    pub fn post_battle(&self) -> Option<&PostBattle> {
        self.post_battle.as_ref().map(|(_, flow)| flow)
    }

    /// The player's party as it is now (with the running post-battle flow's changes);
    /// `None` before the first battle.
    #[must_use]
    pub fn party(&self) -> Option<&PlayerParty> {
        self.post_battle()
            .map(PostBattle::party)
            .or(self.party.as_ref())
    }

    /// Battle content, once the first battle loaded it.
    #[must_use]
    pub fn battle_content(&self) -> Option<&BattleContent> {
        self.battle_content.as_ref()
    }

    /// Reseeds the stream battle RNGs derive from (scenario runs pin it, §15.2).
    pub fn set_battle_seed(&mut self, seed: u64) {
        self.battle_rng = Pcg64Rng::from_seed(seed);
    }

    /// Reseeds the stream wild encounter rolls draw from (scenario runs pin it, §15.2).
    pub fn set_encounter_seed(&mut self, seed: u64) {
        self.encounters.set_seed(seed);
    }

    /// `true` while a warp fade is running (for integration tests).
    #[must_use]
    pub fn is_warping(&self) -> bool {
        !matches!(self.warp_phase, WarpPhase::None)
    }

    /// Overworld scripting, when enabled with [`with_scripts`](Self::with_scripts).
    #[must_use]
    pub fn scripts(&self) -> Option<&OverworldScripts> {
        self.scripts.as_ref()
    }

    /// Mutable overworld scripting (tests, debug console).
    pub fn scripts_mut(&mut self) -> Option<&mut OverworldScripts> {
        self.scripts.as_mut()
    }

    /// The script dialogue on screen, if any.
    #[must_use]
    pub fn overlay(&self) -> Option<&ScriptOverlay> {
        self.overlay.as_ref()
    }

    /// Current screen fade level (`camera.fade_out` / `fade_in`), `0.0`–`1.0`.
    #[must_use]
    pub fn fade_level(&self) -> f32 {
        self.fade.level()
    }

    /// The scene (map, entities, camera).
    #[must_use]
    pub fn scene(&self) -> &MapScene {
        &self.scene
    }

    /// The player walker.
    #[must_use]
    pub fn walker(&self) -> &GridStepper {
        &self.walker
    }

    /// Custom input actions bound so far (`input.register_action`), by name.
    #[must_use]
    pub fn custom_actions(&self) -> Vec<(String, Action)> {
        self.custom_actions.clone()
    }

    /// The input facade (bindings for scripted input §15.2, custom actions §10).
    #[must_use]
    pub fn input(&self) -> &Input {
        &self.input
    }

    /// Shared custom-terrain rule registry (§6.3).
    pub fn terrain_rules_mut(&mut self) -> &mut TerrainRuleRegistry {
        &mut self.terrain_rules
    }

    /// Simulation ticks run so far.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    fn held_direction(&self) -> Option<Direction> {
        [
            (BuiltinAction::Up, Direction::Up),
            (BuiltinAction::Down, Direction::Down),
            (BuiltinAction::Left, Direction::Left),
            (BuiltinAction::Right, Direction::Right),
        ]
        .into_iter()
        .find(|(a, _)| self.input.down(Action::Builtin(*a)))
        .map(|(_, d)| d)
    }

    fn begin_warp(&mut self, warp: &Warp) {
        let from_map = self.scene.map().id.clone();
        let pending = PendingWarp {
            to_map: warp.to_map.clone(),
            to: warp.to,
            facing: warp.facing,
            transition: warp.transition,
        };
        tracing::info!(%from_map, to_map = %pending.to_map, pos = %pending.to, ?warp.transition, "map_exit (warp)");
        let out = transition_ticks_out(warp.transition);
        if out == 0 {
            if self.apply_warp(&pending).is_ok() {
                let inn = transition_ticks_in(pending.transition);
                self.warp_phase = if inn == 0 {
                    WarpPhase::None
                } else {
                    WarpPhase::InFade { remaining: inn }
                };
            }
        } else {
            self.warp_phase = WarpPhase::OutFade {
                remaining: out,
                pending: Box::new(pending),
            };
        }
    }

    fn apply_warp(&mut self, pending: &PendingWarp) -> Result<(), ProjectError> {
        let facing = pending.facing.unwrap_or_else(|| self.walker.facing());
        let map = self.project.load_map_by_id(&pending.to_map)?;
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.exit_map();
        }
        self.enter_loaded_map(map, pending.to, facing)
    }

    /// Makes `map` (already loaded, the previous map already exited) the active map with the
    /// player on `to` facing `facing` (warps, loading a save).
    fn enter_loaded_map(
        &mut self,
        map: Map,
        to: GridPos,
        facing: Direction,
    ) -> Result<(), ProjectError> {
        let textures = upload_map_textures(&self.project, &mut self.renderer, &map)?;
        if !map.grid.in_bounds(to) {
            return Err(ProjectError::StartOutOfBounds {
                map: map.id.clone(),
                pos: to,
                width: map.width,
                height: map.height,
            });
        }
        let mut scene = MapScene::new(map, &textures)?;
        let materials = self
            .materials
            .for_map(scene.map(), &self.project.vfs, &mut self.renderer);
        scene.set_materials(materials);
        scene.place_entity(
            PLAYER_ENTITY,
            to,
            self.player_sprite.0,
            self.player_sprite.1,
            false,
        );
        self.scene = scene;
        self.active_textures = textures;
        self.walker = GridStepper::new(to, facing);
        self.player_tile = to;
        tracing::info!(map = %self.scene.map().id, pos = %to, "map_enter (warp)");
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.enter_map(self.scene.map(), to, facing);
        }
        self.place_npcs();
        self.refresh_neighbors()?;
        Ok(())
    }

    fn collect_layout_sizes(
        project: &Project,
        layout: &WorldLayout,
    ) -> Result<BTreeMap<String, (u32, u32)>, ProjectError> {
        let mut sizes = BTreeMap::new();
        let mut queue: Vec<String> = layout.map_ids().map(str::to_owned).collect();
        while let Some(id) = queue.pop() {
            if sizes.contains_key(&id) {
                continue;
            }
            let map = project.load_map_by_id(&id)?;
            sizes.insert(id.clone(), (map.width, map.height));
            if let Some(links) = layout.links_for(&id) {
                for neighbor in [
                    links.north.as_ref(),
                    links.south.as_ref(),
                    links.east.as_ref(),
                    links.west.as_ref(),
                ]
                .into_iter()
                .flatten()
                {
                    if !sizes.contains_key(&neighbor.map) {
                        queue.push(neighbor.map.clone());
                    }
                }
            }
        }
        Ok(sizes)
    }

    fn bootstrap_world_layout(&mut self) -> Result<(), ProjectError> {
        let layout = WorldLayout::load_optional(&self.project.vfs)?;
        self.crossing = if let Some(layout) = layout {
            let sizes = Self::collect_layout_sizes(&self.project, &layout)?;
            layout.validate_layout(WORLD_RON, &sizes)?;
            let map = self.scene.map();
            Some(CrossingContext {
                layout,
                map_id: map.id.clone(),
                map_size: (map.width, map.height),
                neighbor_sizes: BTreeMap::new(),
            })
        } else {
            None
        };
        self.refresh_neighbors()?;
        Ok(())
    }

    fn refresh_neighbors(&mut self) -> Result<(), ProjectError> {
        let mut reuse = self.drain_neighbor_reuse();
        self.neighbors.clear();
        let Some(crossing) = self.crossing.as_mut() else {
            return Ok(());
        };
        let map = self.scene.map();
        crossing.map_id = map.id.clone();
        crossing.map_size = (map.width, map.height);
        crossing.neighbor_sizes.clear();

        let links = crossing
            .layout
            .links_for(&map.id)
            .cloned()
            .unwrap_or_default();
        let main_size = crossing.map_size;
        for (dir, link) in [
            (Direction::Up, links.north),
            (Direction::Down, links.south),
            (Direction::Left, links.west),
            (Direction::Right, links.east),
        ] {
            let Some(link) = link else { continue };
            let (neighbor_map, neighbor_textures) = if let Some(cached) = reuse.remove(&link.map) {
                cached
            } else {
                let neighbor_map = self.project.load_map_by_id(&link.map)?;
                let neighbor_textures =
                    upload_map_textures(&self.project, &mut self.renderer, &neighbor_map)?;
                (neighbor_map, neighbor_textures)
            };
            crossing
                .neighbor_sizes
                .insert(link.map.clone(), (neighbor_map.width, neighbor_map.height));
            let offset_px = neighbor_pixel_offset(
                dir,
                main_size,
                (neighbor_map.width, neighbor_map.height),
                (neighbor_map.tile_width, neighbor_map.tile_height),
                link.offset,
            );
            self.neighbors.push(neighbor_cache(
                dir,
                neighbor_map,
                neighbor_textures,
                offset_px,
            ));
        }
        Ok(())
    }

    fn drain_neighbor_reuse(&mut self) -> BTreeMap<String, (Map, Vec<TextureHandle>)> {
        let mut reuse = BTreeMap::new();
        for n in self.neighbors.drain(..) {
            reuse.insert(n.map.id.clone(), (n.map, n.textures));
        }
        reuse
    }

    fn apply_seamless_cross(
        &mut self,
        to_map: &str,
        entry: GridPos,
        via: Direction,
    ) -> Result<(), ProjectError> {
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.exit_map();
        }
        let old_map = self.scene.map().clone();
        let old_textures = std::mem::take(&mut self.active_textures);
        let old_id = old_map.id.clone();

        let promoted = self
            .neighbors
            .iter()
            .position(|n| n.dir == via && n.map.id == to_map)
            .map(|idx| self.neighbors.remove(idx));

        let (map, textures) = if let Some(cache) = promoted {
            (cache.map, cache.textures)
        } else {
            let map = self.project.load_map_by_id(to_map)?;
            let textures = upload_map_textures(&self.project, &mut self.renderer, &map)?;
            (map, textures)
        };

        if !map.grid.in_bounds(entry) {
            return Err(ProjectError::StartOutOfBounds {
                map: map.id.clone(),
                pos: entry,
                width: map.width,
                height: map.height,
            });
        }
        let facing = self.walker.facing();
        let mut scene = MapScene::new(map, &textures)?;
        let materials = self
            .materials
            .for_map(scene.map(), &self.project.vfs, &mut self.renderer);
        scene.set_materials(materials);
        scene.place_entity(
            PLAYER_ENTITY,
            entry,
            self.player_sprite.0,
            self.player_sprite.1,
            false,
        );
        self.scene = scene;
        self.active_textures = textures;
        self.walker.relocate(entry);
        self.player_tile = entry;
        tracing::info!(map = %self.scene.map().id, pos = %entry, "map_enter (seamless)");
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.enter_map(self.scene.map(), entry, facing);
        }
        self.place_npcs();

        let mut reuse = self.drain_neighbor_reuse();
        reuse.insert(old_id, (old_map, old_textures));
        if let Some(crossing) = self.crossing.as_mut() {
            let new_id = self.scene.map().id.clone();
            crossing.map_id = new_id.clone();
            crossing.map_size = (self.scene.map().width, self.scene.map().height);
            crossing.neighbor_sizes.clear();
            let links = crossing
                .layout
                .links_for(&new_id)
                .cloned()
                .unwrap_or_default();
            let main_size = crossing.map_size;
            self.neighbors.clear();
            for (dir, link) in [
                (Direction::Up, links.north),
                (Direction::Down, links.south),
                (Direction::Left, links.west),
                (Direction::Right, links.east),
            ] {
                let Some(link) = link else { continue };
                let (neighbor_map, neighbor_textures) =
                    if let Some(cached) = reuse.remove(&link.map) {
                        cached
                    } else {
                        let neighbor_map = self.project.load_map_by_id(&link.map)?;
                        let neighbor_textures =
                            upload_map_textures(&self.project, &mut self.renderer, &neighbor_map)?;
                        (neighbor_map, neighbor_textures)
                    };
                crossing
                    .neighbor_sizes
                    .insert(link.map.clone(), (neighbor_map.width, neighbor_map.height));
                let offset_px = neighbor_pixel_offset(
                    dir,
                    main_size,
                    (neighbor_map.width, neighbor_map.height),
                    (neighbor_map.tile_width, neighbor_map.tile_height),
                    link.offset,
                );
                self.neighbors.push(neighbor_cache(
                    dir,
                    neighbor_map,
                    neighbor_textures,
                    offset_px,
                ));
            }
        }
        Ok(())
    }

    /// Adds a scene entity for every visible NPC and every item ball of the active map.
    fn place_npcs(&mut self) {
        let Some(scripts) = self.scripts.as_ref() else {
            return;
        };
        let (npcs, items): (Vec<_>, Vec<_>) = {
            let world = scripts.world().get();
            let npcs = world
                .npcs
                .iter()
                .filter(|n| n.visible)
                .map(|n| (n.object_id, n.walker.pos(), n.sprite.clone()))
                .collect();
            let items = world
                .entities
                .iter()
                .filter(|e| matches!(e.kind, EntityKind::Item { .. }))
                .map(|e| (e.object_id, e.rect.origin()))
                .collect();
            (npcs, items)
        };
        for (id, pos, sprite) in npcs {
            let (texture, src) = self.npc_sprite_for(sprite.as_deref());
            self.scene.place_entity(id, pos, texture, src, false);
            let material = self.object_material(id);
            self.scene.set_entity_material(id, material);
        }
        let (texture, src) = self.item_sprite;
        for (id, pos) in items {
            self.scene.place_entity(id, pos, texture, src, false);
        }
    }

    /// The material of map object `id` (its `material` property, §5.6), loaded on first use.
    fn object_material(&mut self, id: u32) -> Option<pkrs_render::MaterialHandle> {
        let name = self
            .scene
            .map()
            .objects
            .iter()
            .find(|o| o.id == id)
            .and_then(|o| o.properties.string("material").ok().flatten())
            .map(str::to_owned)?;
        self.materials
            .get(&name, &self.project.vfs, &mut self.renderer)
    }

    /// Sets a global material uniform (§5.6), as `render.set_global_uniform` does from Lua.
    pub fn set_global_uniform(&mut self, name: &str, value: pkrs_render::UniformValue) {
        self.renderer.set_global_uniform(name, value);
    }

    /// The sprite of an NPC: `sprites/<sprite>.png` when it names one that loads, else the
    /// marker. Each id is loaded once.
    fn npc_sprite_for(&mut self, sprite: Option<&str>) -> (TextureHandle, Rect) {
        let Some(id) = sprite else {
            return self.npc_sprite;
        };
        if let Some(found) = self.npc_sprites.get(id) {
            return *found;
        }
        let (tw, th) = (self.scene.map().tile_width, self.scene.map().tile_height);
        let path = format!("sprites/{id}.png");
        let found = match load_sprite(&self.project, &mut self.renderer, &path, tw, th) {
            Ok(Some(found)) => found,
            Ok(None) => {
                tracing::warn!(sprite = id, %path, "NPC sprite not found, using the marker");
                self.npc_sprite
            }
            Err(err) => {
                tracing::warn!(sprite = id, %path, %err, "NPC sprite failed to load, using the marker");
                self.npc_sprite
            }
        };
        self.npc_sprites.insert(id.to_owned(), found);
        found
    }

    /// Moves NPC entities to their walkers' interpolated tiles; hides hidden ones.
    fn sync_npc_sprites(&mut self) {
        let Some(scripts) = self.scripts.as_mut() else {
            return;
        };
        for id in scripts.take_removed_entities() {
            self.scene.remove_entity(id);
        }
        let npcs: Vec<_> = scripts
            .world()
            .get()
            .npcs
            .iter()
            .map(|n| {
                let pos = n.walker.tile_position();
                (n.object_id, pos, n.visible, n.sprite.clone())
            })
            .collect();
        let (tw, th) = (self.scene.map().tile_width, self.scene.map().tile_height);
        for (id, (tx, ty), visible, sprite) in npcs {
            if !visible {
                self.scene.remove_entity(id);
                continue;
            }
            // Looked up every tick so `npc:set_sprite` shows at once (cached after the first load).
            let (texture, src) = self.npc_sprite_for(sprite.as_deref());
            if self.scene.entity_mut(id).is_none() {
                let tile = GridPos::new(tx.round() as i32, ty.round() as i32);
                self.scene.place_entity(id, tile, texture, src, false);
                let material = self.object_material(id);
                self.scene.set_entity_material(id, material);
            }
            if let Some(entity) = self.scene.entity_mut(id) {
                entity.texture = texture;
                entity.src = src;
                entity.pos.set(tx * tw as f32, ty * th as f32);
            }
        }
    }

    /// Applies what scripts asked for: opens the next dialogue, starts a scripted warp, runs
    /// camera commands and turns the player.
    fn apply_script_requests(&mut self) {
        let Some(scripts) = self.scripts.as_mut() else {
            return;
        };
        for request in scripts.host_mut().take_input_actions() {
            let mut keys = Vec::new();
            for name in &request.keys {
                match Key::parse_name(name) {
                    Some(key) => keys.push(key),
                    None => {
                        tracing::warn!(target: "script", action = %request.name, key = %name, "input.register_action: unknown key")
                    }
                }
            }
            match self.input.register_action(&request.name, &keys) {
                Ok(action) => self.custom_actions.push((request.name, action)),
                Err(err) => {
                    tracing::error!(target: "script", action = %request.name, owner = %request.owner, "input.register_action: {err}");
                }
            }
        }
        if let Some(dir) = scripts.take_player_face() {
            self.walker.face(dir);
        }
        if self.overlay.is_none() && self.battle.is_none() {
            if let Some(request) = scripts.take_ui_request() {
                match &self.ui_theme {
                    Some(theme) => {
                        let view = self.renderer.view_size();
                        self.overlay = Some(ScriptOverlay::open(request, theme, view));
                    }
                    None => scripts.close_dialogue(pkrs_script::ScriptValue::Nil),
                }
            }
        }
        for (name, value) in scripts.take_global_uniforms() {
            self.renderer.set_global_uniform(&name, value);
        }
        let commands = scripts.take_camera_commands();
        let warp = scripts.take_warp_request();
        for command in commands {
            self.apply_camera(command);
        }
        if let Some(warp) = warp {
            if matches!(self.warp_phase, WarpPhase::None) {
                self.begin_warp(&Warp {
                    to_map: warp.map,
                    to: warp.to,
                    facing: warp.facing,
                    transition: WarpTransition::Fade,
                });
            } else {
                tracing::warn!(target: "script", map = %warp.map, "map.warp ignored: a warp is already running");
            }
        }
        self.start_requested_battle();
    }

    /// Starts the battle a script task asked for (`battle.start_wild` / `start_trainer`),
    /// once no dialogue or warp is running. A battle that cannot be set up is logged and its
    /// task resumes with `nil`.
    fn start_requested_battle(&mut self) {
        if self.battle.is_some() || self.overlay.is_some() || self.is_warping() {
            return;
        }
        let Some(scripts) = self.scripts.as_mut() else {
            return;
        };
        let Some((task, request)) = scripts.take_battle_request() else {
            return;
        };
        match self.open_battle(&request) {
            Ok(screen) => {
                tracing::info!(target: "battle", ?request, task, "battle started");
                self.battle = Some((Some(task), screen));
            }
            Err(err) => {
                tracing::error!(target: "battle", ?request, "battle cannot start: {err}");
                if let Some(scripts) = self.scripts.as_mut() {
                    scripts.finish_battle(task, ScriptValue::Nil);
                }
            }
        }
    }

    fn open_battle(&mut self, request: &BattleRequest) -> Result<BattleScreen, String> {
        self.ensure_battle_content()?;
        let theme = self
            .ui_theme
            .clone()
            .ok_or_else(|| "no UI theme to draw the battle with".to_owned())?;
        let scripts = self
            .scripts
            .as_ref()
            .ok_or_else(|| "scripting is disabled".to_owned())?;
        let content = self
            .battle_content
            .as_ref()
            .ok_or_else(|| "battle content missing".to_owned())?;
        let policy = RngPolicy::local_from_game_rng(&mut self.battle_rng);
        let text = self.battle_text();
        let mut prepared = content
            .prepare(request, &policy)
            .map_err(|e| e.to_string())?;
        // The session's party replaces `player.ron` from the second battle on.
        let party = match &self.party {
            Some(party) => party.clone(),
            None => PlayerParty::from_side(&prepared.setup.player, content.pack()),
        };
        prepared.setup.player = party
            .battle_side(
                content,
                &prepared.setup.player.name,
                prepared.config.side_size,
            )
            .map_err(|e| e.to_string())?;
        let prepared = prepared.with_text(text);
        self.party = Some(party);
        let bag = scripts.world().get().state.bag().clone();
        BattleScreen::with_scripts(
            prepared,
            policy,
            theme,
            self.renderer.view_size(),
            bag,
            scripts.host(),
        )
        .map_err(|e| e.to_string())
    }

    /// The active language and text tables (§5.5): the script world's (switched by
    /// `game.set_language`), else the host's own.
    #[must_use]
    pub fn localizer(&self) -> Localizer {
        match self.scripts.as_ref() {
            Some(scripts) => scripts.world().get().localizer.clone(),
            None => self.localizer.clone(),
        }
    }

    /// The active text language.
    #[must_use]
    pub fn language(&self) -> String {
        self.localizer().language().to_owned()
    }

    /// Switches every text shown from now on (dialogue, choices, battle screens opened
    /// afterwards) to `language`, one of `[locale] available` (the options API; scripts use
    /// `game.set_language`). The choice is not saved yet.
    pub fn set_language(&mut self, language: &str) -> Result<(), UnknownLanguage> {
        self.localizer.set_language(language)?;
        if let Some(scripts) = self.scripts.as_ref() {
            scripts.world().get_mut().localizer.set_language(language)?;
        }
        tracing::info!(language, "text language switched");
        Ok(())
    }

    /// Battle texts in the active language with the content database's names.
    fn battle_text(&self) -> crate::locale::BattleText {
        crate::locale::BattleText::new(self.localizer(), self.game_data.as_ref())
    }

    /// The project's content database, loaded (and merged with the plugins' content
    /// database, §9.4) on first use; `features.battles = false` skips battle data.
    fn ensure_game_data(&mut self) -> Result<&GameData, String> {
        if self.game_data.is_none() {
            let options = LoadOptions::from_features(&self.project.manifest.features);
            let vfs = &self.project.vfs;
            let data = match self.scripts.as_ref() {
                Some(scripts) => scripts
                    .host()
                    .with_content(|db| load_game_data(vfs, options, Some(db))),
                None => load_game_data(vfs, options, None),
            }
            .map_err(|e| e.to_string())?;
            self.game_data = Some(data);
        }
        self.game_data
            .as_ref()
            .ok_or_else(|| "no content database".to_owned())
    }

    /// The project's battle content, taken from the content database on first use.
    fn ensure_battle_content(&mut self) -> Result<(), String> {
        if self.battle_content.is_none() {
            let content =
                BattleContent::from_data(self.ensure_game_data()?).map_err(|e| e.to_string())?;
            for err in content.check_species() {
                tracing::warn!(target: "battle", "species data: {err}");
            }
            self.battle_content = Some(content);
        }
        Ok(())
    }

    /// Rolls for a wild encounter after the player completed a step onto `pos` (§6.3) and
    /// starts the wild battle on a hit, through the same path as `battle.start_wild`.
    /// Returns `true` when a battle started. Needs scripting (the battle UI and hooks live in
    /// the script host); no roll while a script holds the player or a dialogue is open.
    fn roll_wild_encounter(&mut self, pos: GridPos) -> bool {
        let Some(scripts) = self.scripts.as_ref() else {
            return false;
        };
        if self.battle.is_some() || self.overlay.is_some() || scripts.player_locked() {
            return false;
        }
        if self.encounters.needs_tables(self.scene.map(), pos) {
            if let Err(err) = self.ensure_battle_content() {
                tracing::error!(target: "encounter", "encounters disabled: no battle content: {err}");
                self.encounters.disable();
                return false;
            }
            if let Some(data) = self.game_data.as_ref() {
                self.encounters.ensure_loaded(data);
            }
        }
        let Some(encounter) = self.encounters.on_step(self.scene.map(), pos) else {
            return false;
        };
        let request = wild_battle_request(&encounter);
        match self.open_battle(&request) {
            Ok(screen) => {
                tracing::info!(target: "encounter", ?encounter, "wild encounter");
                self.battle = Some((None, screen));
                true
            }
            Err(err) => {
                tracing::error!(target: "encounter", ?encounter, "wild battle cannot start: {err}");
                false
            }
        }
    }

    /// Runs the battle for one tick; `true` while a battle is running (the overworld does
    /// not update). When it ends, the waiting task resumes next tick with
    /// [`battle_result_value`] and play returns to the overworld.
    fn tick_battle(&mut self) -> bool {
        if self.post_battle.is_some() {
            self.tick_post_battle();
            return true;
        }
        let Some((_, screen)) = self.battle.as_mut() else {
            return false;
        };
        let result = match self.scripts.as_ref() {
            Some(scripts) => screen.update_with_scripts(&mut self.input, scripts.host()),
            None => screen.update(&mut self.input, &mut NullBattleHooks),
        };
        if let Some(result) = result {
            tracing::info!(target: "battle", ?result, "battle ended");
            match self.begin_post_battle(&result) {
                Some(flow) => self.post_battle = Some((result, flow)),
                None => self.end_battle(&result),
            }
        }
        true
    }

    /// The post-battle flow for `result`, if there is anything to show (§7.3).
    fn begin_post_battle(&self, result: &BattleResult) -> Option<PostBattle> {
        let (_, screen) = self.battle.as_ref()?;
        let party = self.party.as_ref()?;
        let content = self.battle_content.as_ref()?;
        let theme = self.ui_theme.clone()?;
        PostBattle::with_text(
            result,
            &screen.exp_earners(),
            party,
            content.pack(),
            theme,
            self.renderer.view_size(),
            self.battle_text(),
        )
    }

    /// One tick of the post-battle flow; when it is done, the party keeps its changes and
    /// the battle ends.
    fn tick_post_battle(&mut self) {
        let Some((_, flow)) = self.post_battle.as_mut() else {
            return;
        };
        if !flow.update(&mut self.input) {
            return;
        }
        if let Some((result, flow)) = self.post_battle.take() {
            self.battle_texts = self.battle_texts.saturating_add(flow.texts_shown());
            self.party = Some(flow.into_party());
            self.end_battle(&result);
        }
    }

    /// Leaves the battle: the waiting task resumes next tick with [`battle_result_value`]
    /// and play returns to the overworld.
    fn end_battle(&mut self, result: &BattleResult) {
        let Some((task, screen)) = self.battle.take() else {
            return;
        };
        self.battle_texts = self.battle_texts.saturating_add(screen.texts_shown());
        // Only a script-started battle has a task waiting on it; step encounters have none.
        if let (Some(task), Some(scripts)) = (task, self.scripts.as_mut()) {
            scripts.finish_battle(task, battle_result_value(result));
        }
    }

    fn apply_camera(&mut self, command: CameraCommand) {
        let (tw, th) = (
            self.scene.map().tile_width as f32,
            self.scene.map().tile_height as f32,
        );
        match command {
            CameraCommand::PanTo { to, ticks } => {
                let x = to.x as f32 * tw + tw / 2.0;
                let y = to.y as f32 * th + th / 2.0;
                self.scene.camera_mut().pan_to(x, y, ticks);
            }
            CameraCommand::Follow(ScriptCameraTarget::Player) => {
                self.scene.camera_mut().follow(CameraTarget::Player);
            }
            CameraCommand::Follow(ScriptCameraTarget::Npc(id)) => {
                let entity = self
                    .scripts
                    .as_ref()
                    .and_then(|s| s.world().get().npc(&id).map(|n| n.object_id));
                match entity {
                    Some(entity) => self.scene.camera_mut().follow(CameraTarget::Entity(entity)),
                    None => {
                        tracing::warn!(target: "script", npc = %id, "camera.follow: no such npc")
                    }
                }
            }
            CameraCommand::Shake { intensity, ticks } => {
                self.scene.camera_mut().shake(intensity as f32, ticks);
            }
            CameraCommand::FadeOut { ticks } => self.fade.start(1.0, ticks),
            CameraCommand::FadeIn { ticks } => self.fade.start(0.0, ticks),
        }
    }

    /// One overworld tick (§3.3 step 3): the open dialogue takes the input; otherwise the
    /// player walks (gated by the script locks), steps fire `step` / `trigger_enter` / warps,
    /// and Confirm interacts.
    fn tick_overworld(&mut self) {
        if let Some(overlay) = self.overlay.as_mut() {
            if let Some(answer) = overlay.update(&mut self.input) {
                self.overlay = None;
                if let Some(scripts) = self.scripts.as_mut() {
                    scripts.close_dialogue(answer);
                }
            }
            return;
        }
        let held = self.held_direction();
        let dir = match &self.scripts {
            Some(scripts) => scripts.gate(held),
            None => held,
        };
        let running = self.input.down(Action::Builtin(BuiltinAction::Run));
        self.walker.set_running(running);
        let link_crossing = self.crossing.as_ref().map(CrossingContext::link_crossing);
        let events = {
            let grid = &self.scene.map().grid;
            match &self.scripts {
                Some(scripts) => {
                    // Snapshot first: plugin terrain rules may call back into the world.
                    // Occupancy (§6.2): visible NPCs and item balls block the player.
                    let (field, occupied, items) = {
                        let world = scripts.world().get();
                        let occupied: Vec<GridPos> = world
                            .npcs
                            .iter()
                            .filter(|n| n.visible)
                            .map(|n| n.walker.pos())
                            .collect();
                        let items: Vec<_> = world
                            .entities
                            .iter()
                            .filter(|e| matches!(e.kind, EntityKind::Item { .. }))
                            .map(|e| e.rect)
                            .collect();
                        (world.field_state(), occupied, items)
                    };
                    *self.walker.field_mut() = field;
                    let blocked =
                        |p: GridPos| occupied.contains(&p) || items.iter().any(|r| r.contains(p));
                    let rules = PlayerTerrain {
                        builtin: &self.terrain_rules,
                        scripts: Some(scripts.host()),
                    };
                    self.walker
                        .tick(dir, grid, &blocked, &rules, link_crossing.as_ref())
                }
                None => {
                    self.walker
                        .tick(dir, grid, &(), &self.terrain_rules, link_crossing.as_ref())
                }
            }
        };
        for event in events {
            match event {
                StepEvent::StepCompleted { pos, facing, .. } => {
                    tracing::trace!(%pos, "step");
                    if let Some(scripts) = self.scripts.as_mut() {
                        scripts.on_player_step(self.player_tile, pos, facing);
                    }
                    self.player_tile = pos;
                    let warp = self.scene.map().warp_at(pos).cloned();
                    if let Some(warp) = warp {
                        self.begin_warp(&warp);
                        return;
                    }
                    if self.roll_wild_encounter(pos) {
                        if let Some(scripts) = self.scripts.as_mut() {
                            scripts.sync_player(pos, facing);
                        }
                        return;
                    }
                }
                StepEvent::CrossedMap { to_map, entry, .. } => {
                    let via = self.walker.facing();
                    if let Err(e) = self.apply_seamless_cross(&to_map, entry, via) {
                        tracing::error!(%e, "seamless crossing failed");
                    } else if let Some(scripts) = self.scripts.as_mut() {
                        let facing = self.walker.facing();
                        scripts.on_player_step(self.player_tile, entry, facing);
                    }
                    return;
                }
                StepEvent::Turned { .. } => {}
            }
        }
        let confirm = Action::Builtin(BuiltinAction::Confirm);
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.sync_player(self.walker.pos(), self.walker.facing());
            if self.walker.is_idle() && !scripts.player_locked() && self.input.triggered(confirm) {
                self.input.consume(confirm);
                scripts.interact();
            }
        }
    }

    /// Advances a running warp transition; `true` while one is in progress (the overworld
    /// does not update during it).
    fn tick_warp_phase(&mut self) -> bool {
        match &mut self.warp_phase {
            WarpPhase::OutFade { remaining, pending } => {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    let pending = (**pending).clone();
                    if let Err(e) = self.apply_warp(&pending) {
                        tracing::error!(%e, "warp failed");
                        self.warp_phase = WarpPhase::None;
                    } else {
                        let inn = transition_ticks_in(pending.transition);
                        self.warp_phase = if inn == 0 {
                            WarpPhase::None
                        } else {
                            WarpPhase::InFade { remaining: inn }
                        };
                    }
                }
                true
            }
            WarpPhase::InFade { remaining } => {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    self.warp_phase = WarpPhase::None;
                }
                true
            }
            WarpPhase::None => false,
        }
    }

    fn sync_player_sprite(&mut self) {
        let (tx, ty) = self.walker.tile_position();
        let (tw, th) = (self.scene.map().tile_width, self.scene.map().tile_height);
        let (texture, src) = self.player_sprite;
        if let Some(player) = self.scene.entity_mut(PLAYER_ENTITY) {
            player.pos.set(tx * tw as f32, ty * th as f32);
            player.texture = texture;
            player.src = src;
        }
    }
}

/// Plugins whose `data/text/` tables load (load order; the enabled list when the plugin set
/// cannot be resolved, which plugin loading reports itself).
fn text_plugins(project: &Project) -> Vec<String> {
    match crate::plugins::plan_project_plugins(project) {
        Ok(plan) => plan.ids().into_iter().map(str::to_owned).collect(),
        Err(_) => project.manifest.plugins.enabled.clone(),
    }
}

fn upload_map_textures(
    project: &Project,
    renderer: &mut Renderer,
    map: &Map,
) -> Result<Vec<TextureHandle>, ProjectError> {
    let mut textures = Vec::with_capacity(map.tilesets.len());
    for ts in &map.tilesets {
        let image = ts
            .image
            .as_ref()
            .ok_or_else(|| ProjectError::TilesetImage(ts.name.clone()))?;
        let bytes = project.vfs.read(&VfsPath::new(&image.path)?)?;
        textures.push(renderer.load_texture(&bytes)?.0);
    }
    Ok(textures)
}

fn load_player_sprite(
    project: &Project,
    renderer: &mut Renderer,
    tw: u32,
    th: u32,
) -> Result<(TextureHandle, Rect), ProjectError> {
    if let Some(sprite) = load_sprite(project, renderer, PLAYER_SPRITE, tw, th)? {
        return Ok(sprite);
    }
    // No sprite in the project: a plain marker the size of one tile.
    marker_sprite(renderer, tw, th, [230, 60, 60, 255])
}

/// The PNG at `path` with its first frame (one tile wide, at most two tiles tall), or `None`
/// when the project has no such file.
fn load_sprite(
    project: &Project,
    renderer: &mut Renderer,
    path: &str,
    tw: u32,
    th: u32,
) -> Result<Option<(TextureHandle, Rect)>, ProjectError> {
    let path = VfsPath::new(path)?;
    if !project.vfs.exists(&path) {
        return Ok(None);
    }
    let bytes = project.vfs.read(&path)?;
    let (tex, w, h) = renderer.load_texture(&bytes)?;
    let fw = w.min(tw.max(1));
    Ok(Some((
        tex,
        Rect::new(0.0, 0.0, fw as f32, h.min(th * 2) as f32),
    )))
}

/// A one-tile solid-colour sprite (player without `sprites/player.png`, NPCs).
fn marker_sprite(
    renderer: &mut Renderer,
    tw: u32,
    th: u32,
    rgba: [u8; 4],
) -> Result<(TextureHandle, Rect), ProjectError> {
    let pixels: Vec<u8> = std::iter::repeat_n(rgba, (tw * th) as usize)
        .flatten()
        .collect();
    let tex = renderer.create_texture(tw, th, &pixels)?;
    Ok((tex, Rect::new(0.0, 0.0, tw as f32, th as f32)))
}

impl SimulationHost for ProjectHost {
    fn poll_input(&mut self, platform: &Platform) {
        self.input.poll(platform);
    }

    fn begin_tick(&mut self) {
        self.input.begin_tick();
    }

    fn simulation_tick(&mut self, _dt: f64) {
        self.ticks += 1;

        // §3.3 step 2: resume script tasks.
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.run_scripts_phase();
        }
        self.apply_script_requests();

        // Custom input actions (§10) become `action` events.
        if let Some(scripts) = self.scripts.as_mut() {
            for (name, action) in &self.custom_actions {
                if self.input.pressed(*action) {
                    scripts.emit_action(name);
                }
            }
        }

        // §3.3 step 3: the battle, a warp transition or the overworld.
        if !self.tick_battle() && !self.tick_warp_phase() {
            self.tick_overworld();
        }
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.tick_npcs(&self.terrain_rules);
        }

        // §3.3 step 5: engine and script events to listeners.
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.flush_events();
        }
        self.apply_script_requests();

        self.sync_player_sprite();
        self.sync_npc_sprites();
        self.fade.tick();
        self.scene.tick();
    }

    fn render(&mut self, platform: &Platform, alpha: f32) {
        let (w, h) = platform.window_size();
        self.renderer.set_time_ticks(self.ticks);
        self.renderer.begin_frame(w, h);
        if let Some((_, screen)) = &self.battle {
            match self.post_battle.as_ref().map(|(_, flow)| flow) {
                Some(flow) if flow.scene().is_some() => flow.render(&mut self.renderer),
                Some(flow) => {
                    screen.render(&mut self.renderer);
                    flow.render(&mut self.renderer);
                }
                None => screen.render(&mut self.renderer),
            }
            self.renderer.end_frame();
            return;
        }
        let backdrops: Vec<&NeighborBackdrop> =
            self.neighbors.iter().map(|n| &n.backdrop).collect();
        self.scene.render(&mut self.renderer, alpha, &backdrops);
        if let (Some(overlay), Some(theme)) = (&self.overlay, &self.ui_theme) {
            overlay.render(&mut self.renderer, theme);
        }
        self.fade.render(&mut self.renderer);
        self.renderer.end_frame();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_world::{GridStepper, LogicalGrid, MovementConfig, TerrainRuleRegistry};

    #[test]
    fn walker_steps_over_sixteen_ticks_and_respects_collision() {
        let mut grid = LogicalGrid::new(3, 1);
        grid.cell_mut(GridPos::new(2, 0)).unwrap().solid = true;
        let rules = TerrainRuleRegistry::new();
        let cfg = MovementConfig {
            tap_turn_ticks: 1,
            walk_step_ticks: STEP_TICKS,
            ..MovementConfig::default()
        };
        let mut w = GridStepper::with_config(GridPos::new(0, 0), Direction::Down, cfg);
        let links = None;
        w.tick(Some(Direction::Right), &grid, &(), &rules, links);
        w.tick(Some(Direction::Right), &grid, &(), &rules, links);
        assert_eq!(w.pos(), GridPos::new(1, 0));
        assert_eq!(w.facing(), Direction::Right);
        assert!(!w.is_idle());
        for _ in 0..STEP_TICKS - 1 {
            w.tick(None, &grid, &(), &rules, links);
        }
        assert!(w.is_idle());
        assert_eq!(w.tile_position(), (1.0, 0.0));
        w.tick(Some(Direction::Right), &grid, &(), &rules, links);
        assert_eq!(w.pos(), GridPos::new(1, 0));
        w.tick(Some(Direction::Up), &grid, &(), &rules, links);
        w.tick(Some(Direction::Up), &grid, &(), &rules, links);
        while !w.is_idle() {
            w.tick(None, &grid, &(), &rules, links);
        }
        assert_eq!((w.pos(), w.facing()), (GridPos::new(1, 0), Direction::Up));
    }

    #[test]
    fn walker_interpolates_mid_step() {
        let grid = LogicalGrid::new(2, 1);
        let rules = TerrainRuleRegistry::new();
        let cfg = MovementConfig {
            tap_turn_ticks: 1,
            walk_step_ticks: STEP_TICKS,
            ..MovementConfig::default()
        };
        let mut w = GridStepper::with_config(GridPos::new(0, 0), Direction::Down, cfg);
        let links = None;
        w.tick(Some(Direction::Right), &grid, &(), &rules, links);
        w.tick(Some(Direction::Right), &grid, &(), &rules, links);
        for _ in 0..7 {
            w.tick(None, &grid, &(), &rules, links);
        }
        assert_eq!(w.tile_position(), (0.5, 0.0));
    }

    #[test]
    fn dev_paths_live_under_saves() {
        let p = DevPaths::for_project(Path::new("game"));
        assert_eq!(p.data_dir(), Path::new("game").join("saves"));
        assert!(p.config_dir().starts_with(Path::new("game").join("saves")));
        assert!(p.logs_dir().starts_with(Path::new("game").join("saves")));
    }
}
