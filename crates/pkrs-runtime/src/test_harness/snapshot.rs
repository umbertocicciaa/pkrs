//! What a scenario sees of a running [`ProjectHost`] (§15.2).
//!
//! The only place the harness reads game state; it goes through the host's public accessors,
//! so changes to how the overworld stores state stay local to this file.

use pkrs_script::scenario::{Placement, ScenarioSnapshot};

use crate::overworld::UiRequest;
use crate::project::ProjectHost;

/// State name while the player walks around.
pub const STATE_OVERWORLD: &str = "Overworld";
/// State name while a text box (`ui.say`, a sign) is open.
pub const STATE_DIALOGUE: &str = "Dialogue";
/// State name while a choice list (`ui.choose`, `ui.yes_no`) is open.
pub const STATE_CHOICE: &str = "Choice";
/// State name during a warp transition.
pub const STATE_WARP: &str = "Warp";

/// The name `state()` / `expect_state` report for `host`: the open dialogue wins, then a
/// running warp, else the overworld.
#[must_use]
pub fn state_name(host: &ProjectHost) -> &'static str {
    match host.overlay().map(|o| o.request()) {
        Some(UiRequest::Say(_)) => STATE_DIALOGUE,
        Some(UiRequest::Choose { .. }) => STATE_CHOICE,
        None if host.is_warping() => STATE_WARP,
        None => STATE_OVERWORLD,
    }
}

/// A [`ScenarioSnapshot`] of `host` after `host.ticks()` ticks; `dialogues_opened` is tracked
/// by the driver.
#[must_use]
pub fn snapshot(host: &ProjectHost, dialogues_opened: u64) -> ScenarioSnapshot {
    let walker = host.walker();
    let mut snap = ScenarioSnapshot {
        tick: host.ticks(),
        state: state_name(host).to_owned(),
        map: Some(host.scene().map().id.clone()),
        player: Some(Placement {
            pos: walker.pos(),
            facing: walker.facing(),
        }),
        player_idle: walker.is_idle(),
        dialogues_opened,
        actions: host.input().action_names().into_iter().collect(),
        ..ScenarioSnapshot::default()
    };
    if let Some(overlay) = host.overlay() {
        match overlay.request() {
            UiRequest::Say(text) => snap.dialogue = Some(text.clone()),
            UiRequest::Choose { prompt, options } => {
                snap.dialogue.clone_from(prompt);
                snap.choices.clone_from(options);
                snap.cursor = overlay.cursor();
            }
        }
    }
    if let Some(scripts) = host.scripts() {
        snap.player_locked = scripts.player_locked();
        let world = scripts.world().get();
        snap.flags.clone_from(&world.flags);
        snap.vars.clone_from(&world.vars);
        snap.items.clone_from(&world.bag);
        snap.npcs = world
            .npcs
            .iter()
            .filter(|n| n.visible)
            .map(|n| {
                (
                    n.id.clone(),
                    Placement {
                        pos: n.walker.pos(),
                        facing: n.walker.facing(),
                    },
                )
            })
            .collect();
    }
    snap
}
