//! The pkrs game player library: fixed-step loop and tick pipeline (§3).

mod ctx;
mod demo;
mod fixed_step;
mod game_host;
mod main_loop;
pub mod scene;
mod state;
mod tick;

pub use ctx::RuntimeCtx;
pub use demo::AnimationDemoHost;
pub use fixed_step::{FixedStepClock, InterpolatedPosition, TICK_DT, lerp_f32};
pub use game_host::RuntimeHost;
pub use main_loop::{
    LoopExit, RuntimeLoopConfig, SimulationHost, StubSimulationHost, block_on_loop, run_loop,
};
pub use scene::{MapScene, PLAYER_ENTITY, SceneEntity, SceneError, WorldTileGrid};
pub use state::{FadeTransition, GameState, StateStack, TitleState, Transition};
pub use tick::{NullTickHooks, TickHooks, TickPhase, run_tick};
