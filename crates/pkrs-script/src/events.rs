//! Overworld events (§9.2): `events.on(name, fn)` listeners, `events.emit(name, payload)`, and
//! the engine events the runtime emits.
//!
//! Events are **queued**, never dispatched mid-call: scripts and the engine append to one FIFO
//! queue ([`ScriptHost::emit_event`]), and the engine flushes it once per tick in the events
//! phase (§3.3 step 5) with [`Scheduler::dispatch_events`]. Events are handled in **emission
//! order**; each one runs its listeners as a priority-ordered hook chain (§9.4: ascending
//! `priority`, then plugin load order, then registration), starting every listener as a
//! scheduler task so it may yield (`ui.say`, `wait`, …) like any cutscene (§9.3).
//!
//! A listener that **returns** before its first yield takes part in the chain: a non-`nil`
//! return value replaces the payload for the listeners after it, and `{ cancel = true }`
//! stops the chain (later listeners of that event are not started). A listener that yields,
//! fails or returns `nil` leaves the payload unchanged. Events emitted while flushing are
//! dispatched in the same flush, after the ones already queued.

use std::collections::BTreeMap;

use pkrs_core::{Direction, GridPos};

use crate::ScriptError;
use crate::chain::Step;
use crate::host::{ScriptHost, ScriptOwner, ScriptValue, TaskId};
use crate::scheduler::{Scheduler, TaskKind, TaskOutcome};

/// Handle returned by `events.on` (and accepted by `events.off`).
pub type ListenerId = u64;

/// Engine events Lua can subscribe to in API v0 (§9.2). Scripts may emit and listen to any
/// other name too (plugin-defined events).
pub const ENGINE_EVENTS: &[&str] = &[
    "game_start",
    "map_enter",
    "map_exit",
    "step",
    "interact",
    "trigger_enter",
];

/// Events flushed per [`Scheduler::dispatch_events`] call before the rest is deferred to the
/// next flush (guards against listeners that emit forever).
pub const MAX_EVENTS_PER_FLUSH: usize = 1024;

/// One queued event: a name and a payload table (or `nil`).
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptEvent {
    /// Event name (`map_enter`, `interact`, or a plugin-defined name).
    pub name: String,
    /// Handed to every listener as its only argument.
    pub payload: ScriptValue,
}

fn payload(entries: impl IntoIterator<Item = (&'static str, ScriptValue)>) -> ScriptValue {
    ScriptValue::Map(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect::<BTreeMap<_, _>>(),
    )
}

fn pos_entries(pos: GridPos) -> [(&'static str, ScriptValue); 2] {
    [
        ("x", ScriptValue::Int(i64::from(pos.x))),
        ("y", ScriptValue::Int(i64::from(pos.y))),
    ]
}

impl ScriptEvent {
    /// Event `name` with `payload`.
    #[must_use]
    pub fn new(name: impl Into<String>, payload: ScriptValue) -> Self {
        Self {
            name: name.into(),
            payload,
        }
    }

    /// `game_start`: once, when a new game or a loaded save begins.
    #[must_use]
    pub fn game_start() -> Self {
        Self::new("game_start", ScriptValue::Nil)
    }

    /// `map_enter { map }`: the map finished loading and its script ran.
    #[must_use]
    pub fn map_enter(map: &str) -> Self {
        Self::new("map_enter", payload([("map", ScriptValue::from(map))]))
    }

    /// `map_exit { map }`: dispatched before the map's handlers are unloaded.
    #[must_use]
    pub fn map_exit(map: &str) -> Self {
        Self::new("map_exit", payload([("map", ScriptValue::from(map))]))
    }

    /// `step { x, y, facing }`: the player completed a step.
    #[must_use]
    pub fn step(pos: GridPos, facing: Direction) -> Self {
        let [x, y] = pos_entries(pos);
        Self::new(
            "step",
            payload([x, y, ("facing", ScriptValue::String(facing.to_string()))]),
        )
    }

    /// `interact { kind = "npc", npc, x, y }`: the player talked to an NPC.
    #[must_use]
    pub fn interact_npc(npc: &str, pos: GridPos) -> Self {
        let [x, y] = pos_entries(pos);
        Self::new(
            "interact",
            payload([
                ("kind", ScriptValue::from("npc")),
                ("npc", ScriptValue::Npc(npc.to_owned())),
                x,
                y,
            ]),
        )
    }

    /// `interact { kind = "sign", id, text, x, y }`: the player read a sign.
    #[must_use]
    pub fn interact_sign(id: &str, text: Option<&str>, pos: GridPos) -> Self {
        let [x, y] = pos_entries(pos);
        let text = text.map_or(ScriptValue::Nil, ScriptValue::from);
        Self::new(
            "interact",
            payload([
                ("kind", ScriptValue::from("sign")),
                ("id", ScriptValue::from(id)),
                ("text", text),
                x,
                y,
            ]),
        )
    }

    /// `interact { kind = "item", id, item, count, x, y }`: the player picked up an item ball
    /// (`id` is the object's persistent key, §6.5).
    #[must_use]
    pub fn interact_item(id: &str, item: &str, count: u32, pos: GridPos) -> Self {
        let [x, y] = pos_entries(pos);
        Self::new(
            "interact",
            payload([
                ("kind", ScriptValue::from("item")),
                ("id", ScriptValue::from(id)),
                ("item", ScriptValue::from(item)),
                ("count", ScriptValue::Int(i64::from(count))),
                x,
                y,
            ]),
        )
    }

    /// `interact { kind = "tile", x, y }`: the player pressed Confirm facing a plain tile.
    #[must_use]
    pub fn interact_tile(pos: GridPos) -> Self {
        let [x, y] = pos_entries(pos);
        Self::new(
            "interact",
            payload([("kind", ScriptValue::from("tile")), x, y]),
        )
    }

    /// `trigger_enter { id, x, y }`: the player stepped into a trigger zone.
    #[must_use]
    pub fn trigger_enter(id: &str, pos: GridPos) -> Self {
        let [x, y] = pos_entries(pos);
        Self::new(
            "trigger_enter",
            payload([("id", ScriptValue::from(id)), x, y]),
        )
    }

    /// How listener tasks for this event run: `interact` and `trigger_enter` lock the player
    /// (§9.3), everything else runs in the background.
    #[must_use]
    pub fn task_kind(&self) -> TaskKind {
        match self.name.as_str() {
            "interact" => TaskKind::Interact,
            "trigger_enter" => TaskKind::Trigger,
            _ => TaskKind::Background,
        }
    }
}

/// A registered listener, as reported by [`ScriptHost::listeners`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListenerInfo {
    /// Handle from `events.on`.
    pub id: ListenerId,
    /// Environment that registered it.
    pub owner: ScriptOwner,
}

/// What one [`Scheduler::dispatch_events`] flush did.
#[derive(Debug, Default)]
pub struct DispatchReport {
    /// Event names dispatched, in order.
    pub events: Vec<String>,
    /// Listener tasks started, in order, with their first outcome.
    pub started: Vec<(TaskId, TaskOutcome)>,
    /// Listeners that failed before their first wait (already logged).
    pub failures: Vec<ScriptError>,
    /// Events left queued because the flush hit [`MAX_EVENTS_PER_FLUSH`].
    pub deferred: usize,
    /// Events whose listener chain a listener stopped with `{ cancel = true }`, in order.
    pub cancelled: Vec<String>,
}

/// How a listener's return value takes part in its event's chain (§9.4): `nil` passes,
/// `{ cancel = true, message = … }` cancels, anything else replaces the payload.
fn chain_step(value: &ScriptValue) -> Step<ScriptValue> {
    match value {
        ScriptValue::Nil => Step::Pass,
        ScriptValue::Map(m) if matches!(m.get("cancel"), Some(ScriptValue::Bool(true))) => {
            let reason = match m.get("message") {
                Some(ScriptValue::String(s)) => Some(s.clone()),
                _ => None,
            };
            Step::Cancel(reason)
        }
        other => Step::Replace(other.clone()),
    }
}

impl Scheduler {
    /// Flushes the host's event queue (§3.3 step 5): events in emission order, and each
    /// event's listeners as a hook chain in [`ScriptHost::listeners`] order (see the module
    /// docs for payload threading and `{ cancel = true }`). Listener failures are logged and
    /// reported; they never stop the other listeners (§9.6).
    pub fn dispatch_events(&mut self, host: &mut (impl ScriptHost + ?Sized)) -> DispatchReport {
        let mut report = DispatchReport::default();
        host.set_tick(self.tick());
        let mut queue = std::collections::VecDeque::from(host.take_events());
        while let Some(event) = queue.pop_front() {
            if report.events.len() >= MAX_EVENTS_PER_FLUSH {
                report.deferred = queue.len() + 1;
                host.emit_event(event);
                for rest in queue.drain(..) {
                    host.emit_event(rest);
                }
                tracing::warn!(
                    target: "script",
                    deferred = report.deferred,
                    "event flush limit reached; the rest is deferred to the next tick"
                );
                break;
            }
            let kind = event.task_kind();
            let name = format!("events:{}", event.name);
            let mut payload = event.payload.clone();
            for listener in host.listeners(&event.name) {
                match host.start_listener(listener.id, std::slice::from_ref(&payload)) {
                    Ok((id, status)) => {
                        let outcome = self.adopt(&listener.owner, &name, kind, id, status);
                        let step = match &outcome {
                            TaskOutcome::Finished(value) => chain_step(value),
                            _ => Step::Pass,
                        };
                        report.started.push((id, outcome));
                        match step {
                            Step::Pass => {}
                            Step::Replace(next) => payload = next,
                            Step::Cancel(_) => {
                                report.cancelled.push(event.name.clone());
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        tracing::warn!(
                            target: "script",
                            event = event.name.as_str(),
                            listener = listener.id,
                            "event listener aborted: {err}"
                        );
                        report.failures.push(err);
                    }
                }
            }
            report.events.push(event.name);
            queue.extend(host.take_events());
        }
        report
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_event_payloads() {
        assert_eq!(ScriptEvent::game_start().payload, ScriptValue::Nil);
        let ScriptValue::Map(m) = ScriptEvent::map_enter("route_1").payload else {
            panic!("map payload");
        };
        assert_eq!(m.get("map"), Some(&ScriptValue::from("route_1")));
        let ScriptValue::Map(m) = ScriptEvent::interact_npc("keeper", GridPos::new(3, 4)).payload
        else {
            panic!("map payload");
        };
        assert_eq!(m.get("npc"), Some(&ScriptValue::Npc("keeper".into())));
        assert_eq!(m.get("y"), Some(&ScriptValue::Int(4)));
        let ScriptValue::Map(m) = ScriptEvent::step(GridPos::new(1, 2), Direction::Left).payload
        else {
            panic!("map payload");
        };
        assert_eq!(m.get("facing"), Some(&ScriptValue::from("left")));
    }

    #[test]
    fn interact_and_trigger_listeners_lock_the_player() {
        assert_eq!(
            ScriptEvent::interact_tile(GridPos::new(0, 0)).task_kind(),
            TaskKind::Interact
        );
        assert_eq!(
            ScriptEvent::trigger_enter("t", GridPos::new(0, 0)).task_kind(),
            TaskKind::Trigger
        );
        assert_eq!(ScriptEvent::game_start().task_kind(), TaskKind::Background);
        assert_eq!(
            ScriptEvent::new("custom", ScriptValue::Nil).task_kind(),
            TaskKind::Background
        );
    }
}
