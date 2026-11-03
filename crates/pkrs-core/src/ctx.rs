//! Traits implemented by the shared [`Ctx`](crate) services bag in `pkrs-runtime`.

use crate::events::EventQueue;
use crate::rng::GameRng;

/// Access to the game RNG port from simulation code.
pub trait RngAccess {
    /// Returns the active game RNG.
    fn rng(&mut self) -> &mut dyn GameRng;
}

/// Access to the per-tick engine event queue.
pub trait EventSink {
    /// Returns the event queue owned by the context.
    fn events(&mut self) -> &mut EventQueue;
}
