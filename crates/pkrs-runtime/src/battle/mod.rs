//! The battle UI (§7.2, §4.1): a scene at internal resolution, action menus and a playback
//! queue for the [`BattleEvent`](pkrs_battle::BattleEvent)s the pure battle logic produces.
//!
//! - [`content`]: builds a battle from the project's `data/` for `battle.start_wild` /
//!   `battle.start_trainer` requests.
//! - [`present`]: maps events to presentation [`Cue`]s (text, HP bar, faint, switch).
//! - [`menu`]: builds the action (incl. hook-added entries), fight, bag and party menus.
//! - [`screen`]: the [`BattleScreen`] that steps the logic on player input and plays the
//!   cues back; [`BattleState`](crate::BattleState) puts it on the state stack, and the
//!   project host runs it as its battle mode.

pub mod content;
pub mod menu;
pub mod present;
pub mod screen;

pub use content::{BattleContent, BattleContentError, BattleKind, PreparedBattle};
pub use menu::{ActionKind, MenuEntry};
pub use present::{Cue, Member, Presenter, Roster, SideRoster};
pub use screen::{BattleScreen, MenuKind};

#[cfg(test)]
mod tests;
