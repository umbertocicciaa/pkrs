//! Running a game project from loose files (`--project <dir>`, used by `pkrs run`, §2.3, §14.1).
//!
//! Loads `pkrs.toml`, mounts the project folder through the assets VFS, imports the start map
//! with the Tiled adapter and draws it with the player at the start position. Dev saves go to
//! `<project>/saves/` (§12).

use std::path::{Path, PathBuf};

use pkrs_assets::{FolderVfs, Vfs, VfsPath};
use pkrs_core::{Direction, GridPos, ProjectManifest, Version};
use pkrs_platform::{
    Action, BuiltinAction, DisplaySettings, Input, InputFacadeConfig, Platform, UserPaths,
};
use pkrs_render::{Rect, Renderer, TextureHandle};
use pkrs_world::{
    GridStepper, Map, MapImporter, MovementConfig, StepEvent, TerrainRuleRegistry, TiledImporter,
    Transition as WarpTransition, Warp,
};

use crate::main_loop::SimulationHost;
use crate::scene::{MapScene, PLAYER_ENTITY};

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
    InFade { remaining: u32     },
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

/// The overworld of a loaded project: map scene, player and input.
pub struct ProjectHost {
    project: Project,
    renderer: Renderer,
    scene: MapScene,
    input: Input,
    terrain_rules: TerrainRuleRegistry,
    walker: GridStepper,
    player_sprite: (TextureHandle, Rect),
    ticks: u64,
    warp_phase: WarpPhase,
}

impl ProjectHost {
    /// Loads the start map of `project` and uploads its textures through `renderer`.
    pub fn new(project: &Project, mut renderer: Renderer) -> Result<Self, ProjectError> {
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
        scene.place_entity(
            PLAYER_ENTITY,
            start,
            player_sprite.0,
            player_sprite.1,
            false,
        );
        let input = Input::from_manifest(InputFacadeConfig::default(), &project.manifest.input)
            .unwrap_or_else(|_| Input::new(InputFacadeConfig::default()));
        Ok(Self {
            project: project.clone(),
            renderer,
            scene,
            input,
            terrain_rules: TerrainRuleRegistry::new(),
            walker: GridStepper::new(start, facing),
            player_sprite,
            ticks: 0,
            warp_phase: WarpPhase::None,
        })
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
        let textures = upload_map_textures(&self.project, &mut self.renderer, &map)?;
        if !map.grid.in_bounds(pending.to) {
            return Err(ProjectError::StartOutOfBounds {
                map: map.id.clone(),
                pos: pending.to,
                width: map.width,
                height: map.height,
            });
        }
        let mut scene = MapScene::new(map, &textures)?;
        scene.place_entity(
            PLAYER_ENTITY,
            pending.to,
            self.player_sprite.0,
            self.player_sprite.1,
            false,
        );
        self.scene = scene;
        self.walker = GridStepper::new(pending.to, facing);
        tracing::info!(map = %self.scene.map().id, pos = %pending.to, "map_enter (warp)");
        Ok(())
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
    let path = VfsPath::new(PLAYER_SPRITE)?;
    if project.vfs.exists(&path) {
        let bytes = project.vfs.read(&path)?;
        let (tex, w, h) = renderer.load_texture(&bytes)?;
        let fw = w.min(tw.max(1));
        return Ok((tex, Rect::new(0.0, 0.0, fw as f32, h.min(th * 2) as f32)));
    }
    // No sprite in the project: a plain marker the size of one tile.
    let rgba: Vec<u8> = std::iter::repeat_n([230, 60, 60, 255], (tw * th) as usize)
        .flatten()
        .collect();
    let tex = renderer.create_texture(tw, th, &rgba)?;
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
                self.scene.tick();
                return;
            }
            WarpPhase::InFade { remaining } => {
                *remaining = remaining.saturating_sub(1);
                if *remaining == 0 {
                    self.warp_phase = WarpPhase::None;
                }
                self.scene.tick();
                return;
            }
            WarpPhase::None => {}
        }

        let dir = self.held_direction();
        let running = self.input.down(Action::Builtin(BuiltinAction::Run));
        self.walker.set_running(running);
        let grid = &self.scene.map().grid;
        for event in self.walker.tick(dir, grid, &(), &self.terrain_rules) {
            if let StepEvent::StepCompleted { pos, .. } = event {
                tracing::trace!(%pos, "step");
                let warp = self.scene.map().warp_at(pos).cloned();
                if let Some(warp) = warp {
                    self.begin_warp(&warp);
                    break;
                }
            }
        }
        self.sync_player_sprite();
        self.scene.tick();
    }

    fn render(&mut self, platform: &Platform, alpha: f32) {
        let (w, h) = platform.window_size();
        self.renderer.begin_frame(w, h);
        self.scene.render(&mut self.renderer, alpha);
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
        w.tick(Some(Direction::Right), &grid, &(), &rules);
        w.tick(Some(Direction::Right), &grid, &(), &rules);
        assert_eq!(w.pos(), GridPos::new(1, 0));
        assert_eq!(w.facing(), Direction::Right);
        assert!(!w.is_idle());
        for _ in 0..STEP_TICKS - 1 {
            w.tick(None, &grid, &(), &rules);
        }
        assert!(w.is_idle());
        assert_eq!(w.tile_position(), (1.0, 0.0));
        w.tick(Some(Direction::Right), &grid, &(), &rules);
        assert_eq!(w.pos(), GridPos::new(1, 0));
        w.tick(Some(Direction::Up), &grid, &(), &rules);
        w.tick(Some(Direction::Up), &grid, &(), &rules);
        while !w.is_idle() {
            w.tick(None, &grid, &(), &rules);
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
        w.tick(Some(Direction::Right), &grid, &(), &rules);
        w.tick(Some(Direction::Right), &grid, &(), &rules);
        for _ in 0..7 {
            w.tick(None, &grid, &(), &rules);
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
