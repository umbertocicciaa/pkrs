//! The pkrs game player library: fixed-step loop and tick pipeline (§3).

mod fixed_step;
mod r#loop;
mod tick;

pub use fixed_step::{FixedStepClock, InterpolatedPosition, TICK_DT, lerp_f32};
pub use r#loop::{
    LoopExit, RuntimeLoopConfig, SimulationHost, StubSimulationHost, block_on_loop, run_loop,
};
pub use tick::{NullTickHooks, TickHooks, TickPhase, run_tick};
