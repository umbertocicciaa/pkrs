//! The pkrs game player library: fixed-step loop and tick pipeline (§3).

pub mod battle;
mod ctx;
pub mod data;
mod default_theme;
mod demo;
pub mod encounters;
pub mod evolution;
mod fixed_step;
mod game_host;
mod main_loop;
pub mod overworld;
pub mod party;
pub mod plugins;
pub mod project;
pub mod scene;
pub mod scripts;
mod state;
pub mod test_harness;
mod tick;
mod ui_assets;
mod ui_theme;
pub mod wiring;

pub use ctx::RuntimeCtx;
pub use default_theme::build_default_ui_theme;
pub use demo::AnimationDemoHost;
pub use fixed_step::{FixedStepClock, InterpolatedPosition, TICK_DT, lerp_f32};
pub use game_host::RuntimeHost;
pub use main_loop::{
    LoopExit, RuntimeLoopConfig, SimulationHost, StubSimulationHost, block_on_loop, run_loop,
};
pub use scene::{MapScene, PLAYER_ENTITY, SceneEntity, SceneError, WorldTileGrid};
pub use state::{
    BattleState, ChoiceState, DialogueState, FadeTransition, GameState, LuaUiState, StateStack,
    TitleState, Transition,
};
pub use tick::{NullTickHooks, TickHooks, TickPhase, run_tick};
pub use ui_assets::build_test_ui_theme;
pub use ui_theme::UiTheme;
pub use wiring::{AdapterSet, PlatformAdapter, RenderAdapter, RunMode};
