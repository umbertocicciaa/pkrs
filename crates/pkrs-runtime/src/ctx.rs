//! Shared simulation context handed to [`GameState`](crate::state::GameState) (§4.2).
//!
//! Fields for assets, audio, input, script, data, and world state are added as those
//! subsystems are wired into the runtime.

/// Borrowed services bag for one simulation tick.
#[derive(Clone, Copy, Debug, Default)]
pub struct RuntimeCtx;
