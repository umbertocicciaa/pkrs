//! Engine event queue (emission order preserved, drained once per tick).

use std::collections::BTreeMap;
use std::string::String;
use std::vec::Vec;

/// Key/value payload attached to an engine event (script-visible subset).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EventPayload(pub BTreeMap<String, String>);

impl EventPayload {
    /// Creates an empty payload.
    pub fn new() -> Self {
        Self(BTreeMap::new())
    }
}

/// A single engine event waiting to be dispatched to listeners.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineEvent {
    /// Event name (e.g. `step`, `map_enter`).
    pub name: String,
    /// Optional structured payload.
    pub payload: EventPayload,
}

/// FIFO queue of engine events emitted during a tick.
#[derive(Clone, Debug, Default)]
pub struct EventQueue {
    pending: Vec<EngineEvent>,
}

impl EventQueue {
    /// Creates an empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Queues an event at the end of the current tick's batch.
    pub fn emit(&mut self, name: impl Into<String>, payload: EventPayload) {
        self.pending.push(EngineEvent {
            name: name.into(),
            payload,
        });
    }

    /// Returns the number of events not yet flushed.
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Returns whether the queue has no pending events.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Removes and returns all pending events in emission order (§3.3 step 5).
    pub fn flush(&mut self) -> Vec<EngineEvent> {
        core::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_preserves_emission_order() {
        let mut q = EventQueue::new();
        q.emit("a", EventPayload::new());
        q.emit("b", EventPayload::new());
        q.emit("c", EventPayload::new());
        let batch = q.flush();
        assert_eq!(
            batch.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert!(q.is_empty());
    }
}
