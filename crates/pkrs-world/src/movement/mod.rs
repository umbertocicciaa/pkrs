//! Grid movement (§6.3).

mod config;
mod events;
mod field;
mod lock;
mod occupancy;
mod path;
mod stepper;
mod terrain;

pub use config::MovementConfig;
pub use events::StepEvent;
pub use field::FieldState;
pub use lock::{LockHolder, PlayerLock};
pub use occupancy::Occupancy;
pub use path::{PathProgress, ScriptedPath};
pub use stepper::GridStepper;
pub use terrain::{TerrainRuleFn, TerrainRuleRegistry, TerrainStepVerdict};
