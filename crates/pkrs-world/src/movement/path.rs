//! Scripted NPC paths (`npc:move({ "up", "up", "left" })`, §9.1, §9.8).
//!
//! Pathing stays in Rust (§9.8): a [`ScriptedPath`] feeds one direction at a time to a
//! [`GridStepper`], so NPCs obey the same collision, occupancy and terrain rules as the player.
//! A step blocked by an entity is retried every tick; after
//! [`ScriptedPath::DEFAULT_MAX_BLOCKED_TICKS`] blocked ticks the path gives up so a cutscene
//! can never hang forever.

use std::collections::VecDeque;

use pkrs_core::Direction;

use crate::LogicalGrid;

use super::events::StepEvent;
use super::occupancy::Occupancy;
use super::stepper::GridStepper;
use super::terrain::TerrainRuleRegistry;

/// Progress of a [`ScriptedPath`] after one tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathProgress {
    /// Still walking (or turning, or waiting for a blocked tile to clear).
    Walking,
    /// Every step was taken and the walker is standing still.
    Finished,
    /// A step stayed blocked for too long; the remaining steps were dropped.
    GaveUp,
}

impl PathProgress {
    /// `true` for [`Finished`](Self::Finished) and [`GaveUp`](Self::GaveUp).
    #[must_use]
    pub const fn is_done(self) -> bool {
        matches!(self, Self::Finished | Self::GaveUp)
    }
}

/// A queue of single-tile steps driven through a [`GridStepper`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptedPath {
    steps: VecDeque<Direction>,
    blocked_ticks: u32,
    max_blocked_ticks: u32,
}

impl ScriptedPath {
    /// Ticks a step may stay blocked before the path gives up (5 s at 60 Hz).
    pub const DEFAULT_MAX_BLOCKED_TICKS: u32 = 300;

    /// Path taking one step per direction, in order.
    #[must_use]
    pub fn new(steps: impl IntoIterator<Item = Direction>) -> Self {
        Self {
            steps: steps.into_iter().collect(),
            blocked_ticks: 0,
            max_blocked_ticks: Self::DEFAULT_MAX_BLOCKED_TICKS,
        }
    }

    /// Overrides how long a blocked step is retried (`0` gives up on the first blocked tick).
    #[must_use]
    pub fn with_max_blocked_ticks(mut self, ticks: u32) -> Self {
        self.max_blocked_ticks = ticks;
        self
    }

    /// Steps not yet completed.
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.steps.len()
    }

    /// Advances `walker` by one tick along the path.
    ///
    /// Returns [`PathProgress::Finished`] once every step completed and the walker is idle.
    pub fn tick<O: Occupancy>(
        &mut self,
        walker: &mut GridStepper,
        grid: &LogicalGrid,
        occupancy: &O,
        rules: &TerrainRuleRegistry,
    ) -> PathProgress {
        let Some(&dir) = self.steps.front() else {
            if walker.is_idle() {
                return PathProgress::Finished;
            }
            walker.tick(None, grid, occupancy, rules, None);
            return if walker.is_idle() {
                PathProgress::Finished
            } else {
                PathProgress::Walking
            };
        };
        let was_idle = walker.is_idle();
        let facing_before = walker.facing();
        let events = walker.tick(Some(dir), grid, occupancy, rules, None);
        let completed = events
            .iter()
            .filter(|e| matches!(e, StepEvent::StepCompleted { .. }))
            .count();
        for _ in 0..completed {
            self.steps.pop_front();
        }
        let started_or_turned = !walker.is_idle() || walker.facing() != facing_before;
        if was_idle && !started_or_turned && completed == 0 {
            // Idle and still idle: either the hold threshold is building up or the tile is
            // blocked. Count it; the tap threshold takes at most a couple of ticks.
            self.blocked_ticks = self.blocked_ticks.saturating_add(1);
            if self.blocked_ticks > self.max_blocked_ticks.saturating_add(2) {
                self.steps.clear();
                return PathProgress::GaveUp;
            }
        } else {
            self.blocked_ticks = 0;
        }
        if self.steps.is_empty() && walker.is_idle() {
            PathProgress::Finished
        } else {
            PathProgress::Walking
        }
    }
}

#[cfg(test)]
mod tests {
    use pkrs_core::GridPos;

    use super::*;
    use crate::MovementConfig;

    fn npc(pos: GridPos) -> GridStepper {
        GridStepper::with_config(
            pos,
            Direction::Down,
            MovementConfig {
                tap_turn_ticks: 1,
                ..MovementConfig::default()
            },
        )
    }

    fn run(
        path: &mut ScriptedPath,
        walker: &mut GridStepper,
        grid: &LogicalGrid,
        occupancy: &impl Occupancy,
        max: u32,
    ) -> (PathProgress, u32) {
        let rules = TerrainRuleRegistry::new();
        for tick in 1..=max {
            let progress = path.tick(walker, grid, occupancy, &rules);
            if progress.is_done() {
                return (progress, tick);
            }
        }
        (PathProgress::Walking, max)
    }

    #[test]
    fn walks_every_step_in_order() {
        let grid = LogicalGrid::new(5, 5);
        let mut walker = npc(GridPos::new(2, 2));
        let mut path = ScriptedPath::new([Direction::Up, Direction::Up, Direction::Left]);
        let (progress, _) = run(&mut path, &mut walker, &grid, &(), 500);
        assert_eq!(progress, PathProgress::Finished);
        assert_eq!(walker.pos(), GridPos::new(1, 0));
        assert_eq!(walker.facing(), Direction::Left);
        assert_eq!(path.remaining(), 0);
    }

    #[test]
    fn empty_path_finishes_immediately() {
        let grid = LogicalGrid::new(2, 2);
        let mut walker = npc(GridPos::new(0, 0));
        let mut path = ScriptedPath::new([]);
        let (progress, ticks) = run(&mut path, &mut walker, &grid, &(), 10);
        assert_eq!((progress, ticks), (PathProgress::Finished, 1));
    }

    #[test]
    fn waits_while_blocked_then_continues() {
        let grid = LogicalGrid::new(3, 1);
        let mut walker = npc(GridPos::new(0, 0));
        let mut path = ScriptedPath::new([Direction::Right]);
        let rules = TerrainRuleRegistry::new();
        let blocked = |p: GridPos| p == GridPos::new(1, 0);
        for _ in 0..50 {
            assert_eq!(
                path.tick(&mut walker, &grid, &blocked, &rules),
                PathProgress::Walking
            );
        }
        assert_eq!(walker.pos(), GridPos::new(0, 0));
        let (progress, _) = run(&mut path, &mut walker, &grid, &(), 100);
        assert_eq!(progress, PathProgress::Finished);
        assert_eq!(walker.pos(), GridPos::new(1, 0));
    }

    #[test]
    fn gives_up_when_blocked_too_long() {
        let mut grid = LogicalGrid::new(2, 1);
        grid.cell_mut(GridPos::new(1, 0)).unwrap().solid = true;
        let mut walker = npc(GridPos::new(0, 0));
        let mut path =
            ScriptedPath::new([Direction::Right, Direction::Right]).with_max_blocked_ticks(10);
        let (progress, _) = run(&mut path, &mut walker, &grid, &(), 100);
        assert_eq!(progress, PathProgress::GaveUp);
        assert_eq!(path.remaining(), 0);
        assert_eq!(walker.pos(), GridPos::new(0, 0));
    }
}
