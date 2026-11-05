//! Typed map objects and encounter rects (§6.2, §6.4, §6.5).

use pkrs_core::{Direction, GridPos};

use super::properties::Properties;

/// A tile-aligned rectangle (`x`, `y` top-left; `width`, `height` in tiles, at least 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileRect {
    /// Left column.
    pub x: i32,
    /// Top row.
    pub y: i32,
    /// Width in tiles.
    pub width: u32,
    /// Height in tiles.
    pub height: u32,
}

impl TileRect {
    /// Creates a rect.
    #[must_use]
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Top-left tile.
    #[must_use]
    pub const fn origin(&self) -> GridPos {
        GridPos::new(self.x, self.y)
    }

    /// `true` when `pos` lies inside the rect.
    #[must_use]
    pub fn contains(&self, pos: GridPos) -> bool {
        let dx = i64::from(pos.x) - i64::from(self.x);
        let dy = i64::from(pos.y) - i64::from(self.y);
        dx >= 0 && dy >= 0 && dx < i64::from(self.width) && dy < i64::from(self.height)
    }
}

/// Visual transition used by a warp (§6.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Transition {
    /// Fade to black and back (default).
    #[default]
    Fade,
    /// Instant swap.
    None,
    /// Door animation, then fade.
    Door,
}

impl Transition {
    /// Parses a `transition=` value.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "fade" => Some(Self::Fade),
            "none" => Some(Self::None),
            "door" => Some(Self::Door),
            _ => None,
        }
    }
}

/// Destination of a `warp` object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warp {
    /// Target map id.
    pub to_map: String,
    /// Target tile.
    pub to: GridPos,
    /// Facing on arrival (`None` = keep current facing).
    pub facing: Option<Direction>,
    /// Transition effect.
    pub transition: Transition,
}

/// What an object on the `objects` layer is, with its required properties validated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObjectKind {
    /// Map change (`to_map`, `to_x`, `to_y`, optional `facing`, `transition`).
    Warp(Warp),
    /// Non-player character (`script`, `sprite`, `facing` optional).
    Npc {
        /// Interaction script (`maps/route1.lua:old_man`).
        script: Option<String>,
        /// Sprite sheet path or id.
        sprite: Option<String>,
        /// Initial facing.
        facing: Direction,
    },
    /// Area trigger (`on_enter` script, `once`).
    Trigger {
        /// Script run when the player steps in.
        on_enter: Option<String>,
        /// Fire at most once (recorded in persistent map state, §6.5).
        once: bool,
    },
    /// Readable sign (`script` or `text` key).
    Sign {
        /// Interaction script.
        script: Option<String>,
        /// Text key shown when there is no script.
        text: Option<String>,
    },
    /// Item ball (`item`, `count` default 1).
    Item {
        /// Item content id.
        item: String,
        /// How many.
        count: u32,
    },
    /// Named arrival point (`facing` optional).
    Spawn {
        /// Facing on arrival.
        facing: Option<Direction>,
    },
}

impl ObjectKind {
    /// The Tiled class name for this kind.
    #[must_use]
    pub fn class_name(&self) -> &'static str {
        match self {
            Self::Warp(_) => "warp",
            Self::Npc { .. } => "npc",
            Self::Trigger { .. } => "trigger",
            Self::Sign { .. } => "sign",
            Self::Item { .. } => "item",
            Self::Spawn { .. } => "spawn",
        }
    }
}

/// One object from the `objects` layer.
#[derive(Clone, Debug, PartialEq)]
pub struct MapObject {
    /// Tiled object id (stable key for persistent map state, §6.5).
    pub id: u32,
    /// Object name (spawn names, script lookups).
    pub name: String,
    /// Tile area covered by the object (1×1 for points and NPCs).
    pub rect: TileRect,
    /// Typed meaning.
    pub kind: ObjectKind,
    /// All custom properties, including the ones consumed by [`kind`](Self::kind) (plugins
    /// may read extra keys).
    pub properties: Properties,
}

impl MapObject {
    /// Top-left tile of the object.
    #[must_use]
    pub fn pos(&self) -> GridPos {
        self.rect.origin()
    }

    /// Stable persistence key: the `uid` property if set, otherwise the Tiled id (§6.5).
    #[must_use]
    pub fn persistent_key(&self) -> String {
        match self.properties.string("uid") {
            Ok(Some(uid)) if !uid.is_empty() => uid.to_owned(),
            _ => self.id.to_string(),
        }
    }
}

/// One rect on the `encounters` layer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncounterZone {
    /// Tiled object id.
    pub id: u32,
    /// Covered tiles.
    pub rect: TileRect,
    /// Encounter table id (`data/encounters/*.ron`).
    pub table: String,
    /// Encounter rate per step (0–100).
    pub rate: u8,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::PropertyValue;

    #[test]
    fn rect_contains_is_half_open() {
        let r = TileRect::new(2, 3, 2, 1);
        assert!(r.contains(GridPos::new(2, 3)));
        assert!(r.contains(GridPos::new(3, 3)));
        assert!(!r.contains(GridPos::new(4, 3)));
        assert!(!r.contains(GridPos::new(2, 4)));
        assert!(!r.contains(GridPos::new(1, 3)));
    }

    #[test]
    fn persistent_key_prefers_uid() {
        let mut obj = MapObject {
            id: 7,
            name: String::new(),
            rect: TileRect::new(0, 0, 1, 1),
            kind: ObjectKind::Spawn { facing: None },
            properties: Properties::new(),
        };
        assert_eq!(obj.persistent_key(), "7");
        obj.properties
            .insert("uid", PropertyValue::String("lamp_oil".into()));
        assert_eq!(obj.persistent_key(), "lamp_oil");
    }

    #[test]
    fn transition_parses_known_values() {
        assert_eq!(Transition::parse("door"), Some(Transition::Door));
        assert_eq!(Transition::parse("wipe"), None);
    }
}
