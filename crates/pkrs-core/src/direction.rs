//! Facing and grid step directions.

use core::fmt;
use core::str::FromStr;

use crate::error::{CoreError, Result};

/// Map-facing direction (orthogonal only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    /// Facing / step toward decreasing `y`.
    Up,
    /// Facing / step toward increasing `y`.
    Down,
    /// Facing / step toward decreasing `x`.
    Left,
    /// Facing / step toward increasing `x`.
    Right,
}

impl Direction {
    /// Returns the opposite facing.
    pub fn opposite(self) -> Self {
        match self {
            Self::Up => Self::Down,
            Self::Down => Self::Up,
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }

    /// Quarter turn counter-clockwise (when viewed from above).
    pub fn turn_left(self) -> Self {
        match self {
            Self::Up => Self::Left,
            Self::Left => Self::Down,
            Self::Down => Self::Right,
            Self::Right => Self::Up,
        }
    }

    /// Quarter turn clockwise.
    pub fn turn_right(self) -> Self {
        match self {
            Self::Up => Self::Right,
            Self::Right => Self::Down,
            Self::Down => Self::Left,
            Self::Left => Self::Up,
        }
    }

    /// Tile delta `(dx, dy)` with `y` increasing downward (screen space).
    pub fn delta(self) -> (i32, i32) {
        match self {
            Self::Up => (0, -1),
            Self::Down => (0, 1),
            Self::Left => (-1, 0),
            Self::Right => (1, 0),
        }
    }
}

impl FromStr for Direction {
    type Err = CoreError;

    fn from_str(raw: &str) -> Result<Self> {
        match raw {
            "up" => Ok(Self::Up),
            "down" => Ok(Self::Down),
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            _ => Err(CoreError::InvalidDirection(raw.to_owned())),
        }
    }
}

impl fmt::Display for Direction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Up => write!(f, "up"),
            Self::Down => write!(f, "down"),
            Self::Left => write!(f, "left"),
            Self::Right => write!(f, "right"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite_and_turns() {
        assert_eq!(Direction::Up.opposite(), Direction::Down);
        assert_eq!(Direction::Up.turn_left(), Direction::Left);
        assert_eq!(Direction::Up.turn_right(), Direction::Right);
    }

    #[test]
    fn from_str_parses_data_literals() {
        assert!(matches!("down".parse::<Direction>(), Ok(Direction::Down)));
        assert!("sideways".parse::<Direction>().is_err());
    }

    #[test]
    fn delta_matches_neighbour_math() {
        assert_eq!(Direction::Right.delta(), (1, 0));
        assert_eq!(Direction::Up.delta(), (0, -1));
    }
}
