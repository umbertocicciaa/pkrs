//! Shared simulation context handed to [`GameState`](crate::state::GameState) (§4.2).

use std::collections::VecDeque;

use pkrs_platform::{Input, InputFacadeConfig};
use pkrs_script::{LuaScriptHost, Scheduler, WakeSignal};

/// Borrowed services bag for one simulation tick.
pub struct RuntimeCtx {
    /// Action facade: [`Input::poll`] each frame, [`Input::begin_tick`] each simulation tick.
    pub input: Input,
    /// Lua host when scripting is enabled (headless tests, full game).
    pub script_host: Option<LuaScriptHost>,
    /// Coroutine scheduler paired with [`Self::script_host`].
    pub scheduler: Option<Scheduler>,
    pub(crate) script_wakes: VecDeque<WakeSignal>,
}

impl Default for RuntimeCtx {
    fn default() -> Self {
        Self {
            input: Input::new(InputFacadeConfig::default()),
            script_host: None,
            scheduler: None,
            script_wakes: VecDeque::new(),
        }
    }
}

impl RuntimeCtx {
    /// Queues a signal for the script scheduler; consumed at the start of the next
    /// [`TickHooks::scripts`](crate::tick::TickHooks::scripts) phase.
    pub fn queue_script_wake(&mut self, signal: WakeSignal) {
        self.script_wakes.push_back(signal);
    }

    /// Takes queued [`WakeSignal`]s (for the script phase).
    pub fn drain_script_wakes(&mut self) -> VecDeque<WakeSignal> {
        std::mem::take(&mut self.script_wakes)
    }
}
