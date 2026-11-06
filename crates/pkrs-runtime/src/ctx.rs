//! Shared simulation context handed to [`GameState`](crate::state::GameState) (§4.2).

use pkrs_platform::{Input, InputFacadeConfig};

/// Borrowed services bag for one simulation tick.
pub struct RuntimeCtx {
    /// Action facade: [`Input::poll`] each frame, [`Input::begin_tick`] each simulation tick.
    pub input: Input,
}

impl Default for RuntimeCtx {
    fn default() -> Self {
        Self {
            input: Input::new(InputFacadeConfig::default()),
        }
    }
}
