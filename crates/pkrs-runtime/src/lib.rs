//! The pkrs game player library: fixed-step loop and tick pipeline (§3).

mod ctx;
mod fixed_step;
mod game_host;
mod main_loop;
mod state;
mod tick;

pub use ctx::RuntimeCtx;
pub use fixed_step::{FixedStepClock, InterpolatedPosition, TICK_DT, lerp_f32};
pub use game_host::RuntimeHost;
pub use main_loop::{
    LoopExit, RuntimeLoopConfig, SimulationHost, StubSimulationHost, block_on_loop, run_loop,
};
pub use state::{FadeTransition, GameState, StateStack, TitleState, Transition};
pub use tick::{NullTickHooks, TickHooks, TickPhase, run_tick};
