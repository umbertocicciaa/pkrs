//! Draw order layers (§5.2).

/// Sprite / tile draw order from back to front.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Layer {
    /// Ground tiles below entities.
    Ground,
    /// Secondary ground decoration.
    GroundDecor,
    /// Sprites sorted by Y within this band.
    Entities,
    /// Overhead tiles (tree tops, roofs).
    Overhead,
    /// Weather and screen-space effects.
    WeatherFx,
    /// UI drawn at internal resolution.
    Ui,
    /// Full-screen fades and wipes.
    Transition,
}
