//! Grid step state machine with interpolation (§6.3).

use pkrs_core::{Direction, GridPos};

use std::collections::BTreeMap;

use crate::{LogicalGrid, Terrain, WorldLayout};

use super::config::MovementConfig;
use super::events::StepEvent;
use super::field::FieldState;
use super::occupancy::Occupancy;
use super::terrain::{TerrainRuleRegistry, TerrainStepVerdict};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motion {
    Idle,
    Stepping { total_ticks: u32 },
    LedgeHop { total_ticks: u32 },
    IceSlide { total_ticks: u32 },
}

/// Resolves seamless map edge crossings (§6.4).
pub struct LinkCrossing<'a> {
    /// Parsed `maps/world.ron`.
    pub layout: &'a WorldLayout,
    /// Active map id.
    pub map_id: &'a str,
    /// Active map size in tiles.
    pub map_size: (u32, u32),
    /// Cached neighbour map sizes by id.
    pub neighbor_sizes: &'a BTreeMap<String, (u32, u32)>,
}

/// Player/NPC grid movement with tap-to-turn, run speed, terrain and occupancy rules.
#[derive(Clone, Debug)]
pub struct GridStepper {
    pos: GridPos,
    facing: Direction,
    from: GridPos,
    progress: u32,
    motion: Motion,
    config: MovementConfig,
    field: FieldState,
    hold_dir: Option<Direction>,
    hold_ticks: u32,
    foot: u8,
    running: bool,
    pending_crossing: Option<(String, GridPos)>,
}

impl GridStepper {
    /// Standing on `pos` facing `facing`.
    #[must_use]
    pub fn new(pos: GridPos, facing: Direction) -> Self {
        Self::with_config(pos, facing, MovementConfig::default())
    }

    /// With explicit movement config.
    #[must_use]
    pub fn with_config(pos: GridPos, facing: Direction, config: MovementConfig) -> Self {
        Self {
            pos,
            facing,
            from: pos,
            progress: config.walk_step_ticks,
            motion: Motion::Idle,
            config: config.sanitized(),
            field: FieldState::default(),
            hold_dir: None,
            hold_ticks: 0,
            foot: 0,
            running: false,
            pending_crossing: None,
        }
    }

    /// Current tile (destination while mid-step).
    #[must_use]
    pub fn pos(&self) -> GridPos {
        self.pos
    }

    /// Current facing.
    #[must_use]
    pub fn facing(&self) -> Direction {
        self.facing
    }

    /// Walk-cycle foot for sprite selection.
    #[must_use]
    pub fn foot(&self) -> u8 {
        self.foot
    }

    /// Mutable field state (Surf, …).
    pub fn field_mut(&mut self) -> &mut FieldState {
        &mut self.field
    }

    /// `true` between completed steps.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        matches!(self.motion, Motion::Idle)
    }

    /// Sets whether the entity is running (shorter step duration).
    pub fn set_running(&mut self, running: bool) {
        self.running = running;
    }

    /// Fractional tile position for rendering interpolation.
    #[must_use]
    pub fn tile_position(&self) -> (f32, f32) {
        let total = match self.motion {
            Motion::Idle => self.config.walk_step_ticks,
            Motion::Stepping { total_ticks }
            | Motion::LedgeHop { total_ticks }
            | Motion::IceSlide { total_ticks } => total_ticks,
        };
        let total = total.max(1);
        let t = (self.progress.min(total)) as f32 / total as f32;
        (
            self.from.x as f32 + (self.pos.x - self.from.x) as f32 * t,
            self.from.y as f32 + (self.pos.y - self.from.y) as f32 * t,
        )
    }

    /// One simulation tick. Returns movement events (`StepCompleted` drives `on_step`).
    ///
    /// `rules` holds plugin [`Terrain::Custom`] handlers (§6.3).
    pub fn tick<O: Occupancy>(
        &mut self,
        dir: Option<Direction>,
        grid: &LogicalGrid,
        occupancy: &O,
        rules: &TerrainRuleRegistry,
        links: Option<&LinkCrossing<'_>>,
    ) -> Vec<StepEvent> {
        if !self.is_idle() {
            if dir.is_none() {
                self.hold_dir = None;
                self.hold_ticks = 0;
            }
            return self.advance_motion(grid, occupancy, rules, links);
        }
        self.tick_idle(dir, grid, occupancy, rules, links)
    }

    fn step_ticks(&self) -> u32 {
        let n = if self.running {
            self.config.run_step_ticks
        } else {
            self.config.walk_step_ticks
        };
        n.max(1)
    }

    fn start_motion(&mut self, to: GridPos, motion: Motion, step_dir: Direction) {
        self.from = self.pos;
        self.pos = to;
        self.facing = step_dir;
        self.motion = motion;
        self.progress = 1;
    }

    fn tick_idle<O: Occupancy>(
        &mut self,
        dir: Option<Direction>,
        grid: &LogicalGrid,
        occupancy: &O,
        rules: &TerrainRuleRegistry,
        links: Option<&LinkCrossing<'_>>,
    ) -> Vec<StepEvent> {
        let Some(dir) = dir else {
            self.hold_dir = None;
            self.hold_ticks = 0;
            return Vec::new();
        };

        if self.hold_dir != Some(dir) {
            self.hold_dir = Some(dir);
            self.hold_ticks = 1;
            if self.facing != dir {
                self.facing = dir;
                return vec![StepEvent::Turned {
                    pos: self.pos,
                    facing: self.facing,
                }];
            }
            return Vec::new();
        }

        self.hold_ticks += 1;
        if self.hold_ticks < self.config.tap_turn_ticks {
            return Vec::new();
        }

        match plan_step(self.pos, dir, grid, &self.field, rules, occupancy, links) {
            StepPlan::Blocked => Vec::new(),
            StepPlan::Normal(to) => {
                let ticks = self.step_ticks();
                self.start_motion(to, Motion::Stepping { total_ticks: ticks }, dir);
                Vec::new()
            }
            StepPlan::LedgeHop(to) => {
                let ticks = self.config.ledge_step_ticks.max(1);
                self.start_motion(to, Motion::LedgeHop { total_ticks: ticks }, dir);
                Vec::new()
            }
            StepPlan::ExitMap {
                to,
                dest_map,
                entry,
            } => {
                let ticks = self.step_ticks();
                self.pending_crossing = Some((dest_map, entry));
                self.start_motion(to, Motion::Stepping { total_ticks: ticks }, dir);
                Vec::new()
            }
        }
    }

    fn advance_motion<O: Occupancy>(
        &mut self,
        grid: &LogicalGrid,
        occupancy: &O,
        rules: &TerrainRuleRegistry,
        links: Option<&LinkCrossing<'_>>,
    ) -> Vec<StepEvent> {
        let total = match self.motion {
            Motion::Stepping { total_ticks }
            | Motion::LedgeHop { total_ticks }
            | Motion::IceSlide { total_ticks } => total_ticks.max(1),
            Motion::Idle => return Vec::new(),
        };
        self.progress += 1;
        if self.progress < total {
            return Vec::new();
        }

        self.progress = total;
        self.foot ^= 1;
        if let Some((to_map, entry)) = self.pending_crossing.take() {
            let exit = self.pos;
            self.motion = Motion::Idle;
            self.progress = self.config.walk_step_ticks;
            return vec![StepEvent::CrossedMap {
                to_map,
                entry,
                exit,
            }];
        }

        let event = StepEvent::StepCompleted {
            pos: self.pos,
            facing: self.facing,
            foot: self.foot,
        };

        if matches!(grid.terrain(self.pos), Some(Terrain::Ice)) {
            let dir = self.facing;
            if let StepPlan::Normal(to) =
                plan_step(self.pos, dir, grid, &self.field, rules, occupancy, links)
            {
                let ticks = self.step_ticks();
                self.start_motion(to, Motion::IceSlide { total_ticks: ticks }, dir);
                return vec![event];
            }
        }

        self.motion = Motion::Idle;
        self.progress = self.config.walk_step_ticks;
        vec![event]
    }
}

enum StepPlan {
    Blocked,
    Normal(GridPos),
    LedgeHop(GridPos),
    ExitMap {
        to: GridPos,
        dest_map: String,
        entry: GridPos,
    },
}

fn plan_step<O: Occupancy>(
    from: GridPos,
    dir: Direction,
    grid: &LogicalGrid,
    field: &FieldState,
    rules: &TerrainRuleRegistry,
    occupancy: &O,
    links: Option<&LinkCrossing<'_>>,
) -> StepPlan {
    if !grid.in_bounds(from) {
        return StepPlan::Blocked;
    }

    let to = from.neighbour(dir);

    if !grid.in_bounds(to) {
        if let Some(link) = links {
            let Some(edge) = link.layout.links_for(link.map_id) else {
                return StepPlan::Blocked;
            };
            let Some(neighbor) = edge.link(dir) else {
                return StepPlan::Blocked;
            };
            let Some(to_size) = link.neighbor_sizes.get(&neighbor.map).copied() else {
                return StepPlan::Blocked;
            };
            if let Some((_, entry)) =
                link.layout
                    .crossing(link.map_id, from, dir, link.map_size, to_size)
            {
                let can_exit = grid
                    .cell(from)
                    .is_some_and(|c| !c.blocked_edges.blocks(dir));
                if can_exit {
                    return StepPlan::ExitMap {
                        to,
                        dest_map: neighbor.map.clone(),
                        entry,
                    };
                }
            }
        }
        return StepPlan::Blocked;
    }

    if let Some(Terrain::Ledge(ledge_dir)) = grid.terrain(to) {
        if dir != *ledge_dir {
            return StepPlan::Blocked;
        }
        let dest = to.neighbour(dir);
        if !grid.in_bounds(dest) {
            return StepPlan::Blocked;
        }
        if !grid.can_cross(from, dir) || !grid.can_cross(to, dir) {
            return StepPlan::Blocked;
        }
        if occupancy.blocks(dest) {
            return StepPlan::Blocked;
        }
        if !terrain_allows_at(from, to, dir, grid, field, rules)
            || !terrain_allows_at(to, dest, dir, grid, field, rules)
        {
            return StepPlan::Blocked;
        }
        return StepPlan::LedgeHop(dest);
    }

    if !grid.can_cross(from, dir) || occupancy.blocks(to) {
        return StepPlan::Blocked;
    }
    if !terrain_allows_at(from, to, dir, grid, field, rules) {
        return StepPlan::Blocked;
    }
    StepPlan::Normal(to)
}

fn terrain_allows_at(
    from: GridPos,
    dest: GridPos,
    dir: Direction,
    grid: &LogicalGrid,
    field: &FieldState,
    rules: &TerrainRuleRegistry,
) -> bool {
    let Some(terrain) = grid.terrain(dest) else {
        return true;
    };
    match terrain {
        Terrain::Water if !field.surf => false,
        Terrain::Custom(name) => rules
            .get(name)
            .is_none_or(|rule| rule(field, from, dir) != TerrainStepVerdict::Block),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_step(s: &mut GridStepper, grid: &LogicalGrid, rules: &TerrainRuleRegistry) {
        while !s.is_idle() {
            s.tick(None, grid, &(), rules, None);
        }
    }

    #[test]
    fn tap_turns_without_crossing_tap_threshold() {
        let grid = LogicalGrid::new(3, 3);
        let rules = TerrainRuleRegistry::new();
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 4,
                ..MovementConfig::default()
            },
        );
        let events = s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StepEvent::Turned { .. }));
        assert_eq!(s.pos(), GridPos::new(1, 1));
        for _ in 0..2 {
            s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        }
        assert_eq!(s.pos(), GridPos::new(1, 1));
    }

    #[test]
    fn hold_threshold_starts_step() {
        let grid = LogicalGrid::new(3, 3);
        let rules = TerrainRuleRegistry::new();
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 2,
                walk_step_ticks: 4,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        assert!(!s.is_idle());
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(1, 0));
    }

    #[test]
    fn water_blocks_without_surf() {
        let mut grid = LogicalGrid::new(3, 3);
        grid.cell_mut(GridPos::new(1, 0)).unwrap().terrain = Some(Terrain::Water);
        let rules = TerrainRuleRegistry::new();
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(1, 1));
        s.field_mut().surf = true;
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(1, 0));
    }

    #[test]
    fn ledge_hop_from_tile_before_ledge() {
        let mut grid = LogicalGrid::new(5, 5);
        grid.cell_mut(GridPos::new(2, 2)).unwrap().terrain = Some(Terrain::Ledge(Direction::Down));
        let rules = TerrainRuleRegistry::new();
        let mut s = GridStepper::with_config(
            GridPos::new(2, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ledge_step_ticks: 4,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Down), &grid, &(), &rules, None);
        s.tick(Some(Direction::Down), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(2, 3));
    }

    #[test]
    fn ledge_blocks_wrong_entry_direction() {
        let mut grid = LogicalGrid::new(5, 5);
        grid.cell_mut(GridPos::new(2, 2)).unwrap().terrain = Some(Terrain::Ledge(Direction::Down));
        let rules = TerrainRuleRegistry::new();
        let mut s = GridStepper::with_config(
            GridPos::new(2, 3),
            Direction::Up,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(2, 3));
    }

    #[test]
    fn ice_slide_continues_until_blocked() {
        let mut grid = LogicalGrid::new(6, 2);
        for x in 1..4 {
            grid.cell_mut(GridPos::new(x, 0)).unwrap().terrain = Some(Terrain::Ice);
        }
        grid.cell_mut(GridPos::new(4, 0)).unwrap().solid = true;
        let rules = TerrainRuleRegistry::new();
        let mut s = GridStepper::with_config(
            GridPos::new(0, 0),
            Direction::Right,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Right), &grid, &(), &rules, None);
        s.tick(Some(Direction::Right), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(3, 0));
    }

    #[test]
    fn custom_terrain_rule_sees_entry_direction() {
        let mut grid = LogicalGrid::new(4, 3);
        grid.cell_mut(GridPos::new(1, 0)).unwrap().terrain = Some(Terrain::Custom("mud".into()));
        grid.cell_mut(GridPos::new(2, 1)).unwrap().terrain = Some(Terrain::Custom("mud".into()));
        let mut rules = TerrainRuleRegistry::new();
        rules.register("mud", |_field, _from, dir| {
            if dir == Direction::Right {
                TerrainStepVerdict::Allow
            } else {
                TerrainStepVerdict::Block
            }
        });
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ..MovementConfig::default()
            },
        );
        // Into mud at (1,0) from (1,1): verdict uses step direction (Up) → block.
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        s.tick(Some(Direction::Up), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(1, 1));
        // Into mud at (2,1) from (1,1): direction Right → allow.
        s.tick(Some(Direction::Right), &grid, &(), &rules, None);
        s.tick(Some(Direction::Right), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(2, 1));
        s.tick(Some(Direction::Right), &grid, &(), &rules, None);
        s.tick(Some(Direction::Right), &grid, &(), &rules, None);
        finish_step(&mut s, &grid, &rules);
        assert_eq!(s.pos(), GridPos::new(3, 1));
    }

    #[test]
    fn held_walk_chains_after_first_tap_delay() {
        let grid = LogicalGrid::new(10, 1);
        let rules = TerrainRuleRegistry::new();
        let cfg = MovementConfig {
            tap_turn_ticks: 8,
            walk_step_ticks: 16,
            ..MovementConfig::default()
        };
        let mut s = GridStepper::with_config(GridPos::new(0, 0), Direction::Right, cfg);
        let mut t = 0u32;
        let target_x = 3i32;
        while s.pos().x < target_x {
            s.tick(Some(Direction::Right), &grid, &(), &rules, None);
            t += 1;
            assert!(t < 200, "loop runaway");
        }
        // `pos` jumps to the destination tile when a step starts; the loop stops when x reaches the
        // target, without counting animation ticks on the last tile. First move pays
        // `tap_turn_ticks` to commit; each tile crossed after that adds one step duration.
        let steps = target_x as u32;
        let expected = cfg.tap_turn_ticks + (steps - 1) * cfg.walk_step_ticks;
        assert_eq!(t, expected);
    }
}
