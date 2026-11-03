//! Per-tick update order (§3.3); subsystems are stubs until later milestones.

/// One stage of the deterministic tick pipeline (§3.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickPhase {
    /// Input → action state (§10).
    InputActions,
    /// Script scheduler: resume waiting coroutines.
    Scripts,
    /// State stack: `top.update`.
    StateStack,
    /// Apply deferred push/pop/replace after updates.
    DeferredTransitions,
    /// Flush engine events to script listeners.
    Events,
    /// Flush queued audio commands to the backend.
    AudioFlush,
}

const ORDER: [TickPhase; 6] = [
    TickPhase::InputActions,
    TickPhase::Scripts,
    TickPhase::StateStack,
    TickPhase::DeferredTransitions,
    TickPhase::Events,
    TickPhase::AudioFlush,
];

/// Hooks invoked in [`TickPhase`] order each simulation tick.
pub trait TickHooks {
    /// Input → action state.
    fn input_actions(&mut self, _dt: f64) {}
    /// Script scheduler step.
    fn scripts(&mut self, _dt: f64) {}
    /// Active state stack update.
    fn state_stack(&mut self, _dt: f64) {}
    /// Deferred transitions after updates.
    fn deferred_transitions(&mut self) {}
    /// Event queue dispatch.
    fn events(&mut self) {}
    /// Audio command flush.
    fn audio_flush(&mut self) {}
}

/// Runs one simulation tick through the §3.3 pipeline.
pub fn run_tick(hooks: &mut dyn TickHooks, dt: f64) {
    for phase in ORDER {
        match phase {
            TickPhase::InputActions => hooks.input_actions(dt),
            TickPhase::Scripts => hooks.scripts(dt),
            TickPhase::StateStack => hooks.state_stack(dt),
            TickPhase::DeferredTransitions => hooks.deferred_transitions(),
            TickPhase::Events => hooks.events(),
            TickPhase::AudioFlush => hooks.audio_flush(),
        }
    }
}

/// No-op hooks for headless smoke runs.
#[derive(Clone, Copy, Debug, Default)]
pub struct NullTickHooks;

impl TickHooks for NullTickHooks {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fixed_step::TICK_DT;

    #[derive(Default)]
    struct RecordingHooks {
        log: Vec<TickPhase>,
    }

    impl TickHooks for RecordingHooks {
        fn input_actions(&mut self, _: f64) {
            self.log.push(TickPhase::InputActions);
        }
        fn scripts(&mut self, _: f64) {
            self.log.push(TickPhase::Scripts);
        }
        fn state_stack(&mut self, _: f64) {
            self.log.push(TickPhase::StateStack);
        }
        fn deferred_transitions(&mut self) {
            self.log.push(TickPhase::DeferredTransitions);
        }
        fn events(&mut self) {
            self.log.push(TickPhase::Events);
        }
        fn audio_flush(&mut self) {
            self.log.push(TickPhase::AudioFlush);
        }
    }

    #[test]
    fn tick_runs_phases_in_design_order() {
        let mut hooks = RecordingHooks::default();
        run_tick(&mut hooks, TICK_DT);
        assert_eq!(hooks.log, ORDER.to_vec());
    }
}
