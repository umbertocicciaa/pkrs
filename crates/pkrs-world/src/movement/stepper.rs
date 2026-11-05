//! Grid step state machine with interpolation (§6.3).

use pkrs_core::{Direction, GridPos};

use crate::{LogicalGrid, Terrain};

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
    rules: TerrainRuleRegistry,
    hold_dir: Option<Direction>,
    hold_ticks: u32,
    foot: u8,
    running: bool,
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
            config,
            field: FieldState::default(),
            rules: TerrainRuleRegistry::new(),
            hold_dir: None,
            hold_ticks: 0,
            foot: 0,
            running: false,
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

    /// Custom terrain rules registry.
    pub fn terrain_rules_mut(&mut self) -> &mut TerrainRuleRegistry {
        &mut self.rules
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
        let t = (self.progress.min(total)) as f32 / total as f32;
        (
            self.from.x as f32 + (self.pos.x - self.from.x) as f32 * t,
            self.from.y as f32 + (self.pos.y - self.from.y) as f32 * t,
        )
    }

    /// One simulation tick. Returns movement events (`StepCompleted` drives `on_step`).
    pub fn tick<O: Occupancy>(
        &mut self,
        dir: Option<Direction>,
        grid: &LogicalGrid,
        occupancy: &O,
    ) -> Vec<StepEvent> {
        if !self.is_idle() {
            return self.advance_motion(grid, occupancy);
        }
        self.tick_idle(dir, grid, occupancy)
    }

    fn step_ticks(&self) -> u32 {
        if self.running {
            self.config.run_step_ticks
        } else {
            self.config.walk_step_ticks
        }
    }

    fn start_motion(&mut self, to: GridPos, motion: Motion) {
        self.from = self.pos;
        self.pos = to;
        self.facing = direction_between(self.from, to).unwrap_or(self.facing);
        self.motion = motion;
        self.progress = 1;
        self.hold_dir = None;
        self.hold_ticks = 0;
    }

    fn tick_idle<O: Occupancy>(
        &mut self,
        dir: Option<Direction>,
        grid: &LogicalGrid,
        occupancy: &O,
    ) -> Vec<StepEvent> {
        let Some(dir) = dir else {
            self.hold_dir = None;
            self.hold_ticks = 0;
            return Vec::new();
        };

        if self.hold_dir != Some(dir) {
            self.facing = dir;
            self.hold_dir = Some(dir);
            self.hold_ticks = 1;
            return vec![StepEvent::Turned {
                pos: self.pos,
                facing: self.facing,
            }];
        }

        self.hold_ticks += 1;
        if self.hold_ticks < self.config.tap_turn_ticks {
            return Vec::new();
        }

        match plan_step(self.pos, dir, grid, &self.field, &self.rules, occupancy) {
            StepPlan::Blocked => Vec::new(),
            StepPlan::Normal(to) => {
                let ticks = self.step_ticks();
                self.start_motion(to, Motion::Stepping { total_ticks: ticks });
                Vec::new()
            }
            StepPlan::LedgeHop(to) => {
                let ticks = self.config.ledge_step_ticks;
                self.start_motion(to, Motion::LedgeHop { total_ticks: ticks });
                Vec::new()
            }
        }
    }

    fn advance_motion<O: Occupancy>(
        &mut self,
        grid: &LogicalGrid,
        occupancy: &O,
    ) -> Vec<StepEvent> {
        let total = match self.motion {
            Motion::Stepping { total_ticks }
            | Motion::LedgeHop { total_ticks }
            | Motion::IceSlide { total_ticks } => total_ticks,
            Motion::Idle => return Vec::new(),
        };
        self.progress += 1;
        if self.progress < total {
            return Vec::new();
        }

        self.progress = total;
        self.foot ^= 1;
        let event = StepEvent::StepCompleted {
            pos: self.pos,
            facing: self.facing,
            foot: self.foot,
        };

        if matches!(grid.terrain(self.pos), Some(Terrain::Ice)) {
            let dir = self.facing;
            if let StepPlan::Normal(to) =
                plan_step(self.pos, dir, grid, &self.field, &self.rules, occupancy)
            {
                let ticks = self.step_ticks();
                self.start_motion(to, Motion::IceSlide { total_ticks: ticks });
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
}

fn plan_step<O: Occupancy>(
    from: GridPos,
    dir: Direction,
    grid: &LogicalGrid,
    field: &FieldState,
    rules: &TerrainRuleRegistry,
    occupancy: &O,
) -> StepPlan {
    if !grid.in_bounds(from) {
        return StepPlan::Blocked;
    }

    if let Some(Terrain::Ledge(ledge_dir)) = grid.terrain(from) {
        if dir != *ledge_dir {
            return StepPlan::Blocked;
        }
        let mid = from.neighbour(dir);
        let dest = mid.neighbour(dir);
        if !grid.can_cross(from, dir) || !grid.can_cross(mid, dir) {
            return StepPlan::Blocked;
        }
        if occupancy.blocks(mid) || occupancy.blocks(dest) {
            return StepPlan::Blocked;
        }
        if !terrain_allows_step(from, dir, grid, field, rules)
            || !terrain_allows_at(dest, grid, field, rules)
        {
            return StepPlan::Blocked;
        }
        return StepPlan::LedgeHop(dest);
    }

    let to = from.neighbour(dir);
    if !grid.can_cross(from, dir) || occupancy.blocks(to) {
        return StepPlan::Blocked;
    }
    if !terrain_allows_step(from, dir, grid, field, rules) {
        return StepPlan::Blocked;
    }
    StepPlan::Normal(to)
}

fn terrain_allows_step(
    from: GridPos,
    dir: Direction,
    grid: &LogicalGrid,
    field: &FieldState,
    rules: &TerrainRuleRegistry,
) -> bool {
    terrain_allows_at(from.neighbour(dir), grid, field, rules)
}

fn terrain_allows_at(
    dest: GridPos,
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
            .is_none_or(|rule| rule(field, dest, Direction::Down) != TerrainStepVerdict::Block),
        _ => true,
    }
}

fn direction_between(from: GridPos, to: GridPos) -> Option<Direction> {
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    match (dx, dy) {
        (0, -1) => Some(Direction::Up),
        (0, 1) => Some(Direction::Down),
        (-1, 0) => Some(Direction::Left),
        (1, 0) => Some(Direction::Right),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finish_step(s: &mut GridStepper, grid: &LogicalGrid) {
        while !s.is_idle() {
            s.tick(None, grid, &());
        }
    }

    #[test]
    fn tap_turns_without_crossing_tap_threshold() {
        let grid = LogicalGrid::new(3, 3);
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 4,
                ..MovementConfig::default()
            },
        );
        let events = s.tick(Some(Direction::Up), &grid, &());
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], StepEvent::Turned { .. }));
        assert_eq!(s.pos(), GridPos::new(1, 1));
        for _ in 0..2 {
            s.tick(Some(Direction::Up), &grid, &());
        }
        assert_eq!(s.pos(), GridPos::new(1, 1));
    }

    #[test]
    fn hold_threshold_starts_step() {
        let grid = LogicalGrid::new(3, 3);
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 2,
                walk_step_ticks: 4,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Up), &grid, &());
        s.tick(Some(Direction::Up), &grid, &());
        assert!(!s.is_idle());
        finish_step(&mut s, &grid);
        assert_eq!(s.pos(), GridPos::new(1, 0));
    }

    #[test]
    fn water_blocks_without_surf() {
        let mut grid = LogicalGrid::new(3, 3);
        grid.cell_mut(GridPos::new(1, 0)).unwrap().terrain = Some(Terrain::Water);
        let mut s = GridStepper::with_config(
            GridPos::new(1, 1),
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Up), &grid, &());
        finish_step(&mut s, &grid);
        assert_eq!(s.pos(), GridPos::new(1, 1));
        s.field_mut().surf = true;
        s.tick(Some(Direction::Up), &grid, &());
        finish_step(&mut s, &grid);
        assert_eq!(s.pos(), GridPos::new(1, 0));
    }

    #[test]
    fn ledge_hops_two_tiles() {
        let mut grid = LogicalGrid::new(5, 5);
        grid.cell_mut(GridPos::new(2, 1)).unwrap().terrain = Some(Terrain::Ledge(Direction::Down));
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
        s.tick(Some(Direction::Down), &grid, &());
        s.tick(Some(Direction::Down), &grid, &());
        finish_step(&mut s, &grid);
        assert_eq!(s.pos(), GridPos::new(2, 3));
    }

    #[test]
    fn ice_slide_continues_until_blocked() {
        let mut grid = LogicalGrid::new(6, 2);
        for x in 1..4 {
            grid.cell_mut(GridPos::new(x, 0)).unwrap().terrain = Some(Terrain::Ice);
        }
        grid.cell_mut(GridPos::new(4, 0)).unwrap().solid = true;
        let mut s = GridStepper::with_config(
            GridPos::new(0, 0),
            Direction::Right,
            MovementConfig {
                tap_turn_ticks: 1,
                walk_step_ticks: 2,
                ..MovementConfig::default()
            },
        );
        s.tick(Some(Direction::Right), &grid, &());
        s.tick(Some(Direction::Right), &grid, &());
        finish_step(&mut s, &grid);
        assert_eq!(s.pos(), GridPos::new(3, 0));
    }
}
