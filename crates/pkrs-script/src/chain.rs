//! Priority-ordered hook chains (§9.4 "Hook combination").
//!
//! Engine-agnostic and event-agnostic: the same machinery orders and runs `battle.on`
//! handlers (§7.4) and world-event handlers (§9.2).
//!
//! - **Order.** Handlers run by [`ChainKey`]: ascending `priority` (default `0`), then plugin
//!   load order ([`LoadOrder`]), then registration order. The same plugins in the same order
//!   therefore always give the same chain.
//! - **Value threading.** [`run_chain`] hands every handler the current value; a handler
//!   returns [`Step::Pass`] (Lua `nil`: unchanged), [`Step::Replace`] (the new value for the
//!   rest of the chain) or [`Step::Cancel`] (`{ cancel = true, message = … }`), which stops
//!   the chain. Hooks whose payload is mutated in place use `V = ()`.
//! - **Introspection.** [`HookTable`] is the resolved table `pkrs check --plugins` prints.

use core::fmt;
use std::collections::BTreeMap;

use crate::host::ScriptOwner;

/// Sort key of one handler in a chain. Ordered by `priority`, then `rank`, then `seq`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChainKey {
    /// Handler priority from `{ priority = n }` (lower runs first).
    pub priority: i64,
    /// Owner's position in plugin load order ([`LoadOrder::rank`]).
    pub rank: u32,
    /// Global registration sequence number (unique per handler).
    pub seq: u64,
}

impl ChainKey {
    /// Key from its three parts.
    #[must_use]
    pub fn new(priority: i64, rank: u32, seq: u64) -> Self {
        Self {
            priority,
            rank,
            seq,
        }
    }
}

/// Plugin load order, as seen by hook chains.
///
/// Plugins are ranked in the order they are bound ([`load_plugins`](crate::load_plugins) binds
/// them in resolved load order). Plugins never bound (e.g. scripts run directly under a plugin
/// owner in tests) rank after every bound plugin, and the game's own scripts rank last: they
/// run after plugins at start-up, so at equal priority the game has the final say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LoadOrder {
    ranks: BTreeMap<String, u32>,
}

impl LoadOrder {
    /// Rank of [`ScriptOwner::Game`] and of per-map scripts ([`ScriptOwner::Map`]): game
    /// content runs after every plugin at equal priority.
    pub const GAME_RANK: u32 = u32::MAX;
    /// Rank of a plugin owner that was never [`bind`](Self::bind)-ed.
    pub const UNBOUND_RANK: u32 = u32::MAX - 1;

    /// Empty order.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Gives plugin `id` the next rank. Binding an id again keeps its first rank.
    pub fn bind(&mut self, id: &str) -> u32 {
        let next = u32::try_from(self.ranks.len())
            .unwrap_or(Self::UNBOUND_RANK)
            .min(Self::UNBOUND_RANK - 1);
        *self.ranks.entry(id.to_owned()).or_insert(next)
    }

    /// Rank of `owner` (see the type docs).
    #[must_use]
    pub fn rank(&self, owner: &ScriptOwner) -> u32 {
        match owner {
            ScriptOwner::Game | ScriptOwner::Map(_) => Self::GAME_RANK,
            ScriptOwner::Plugin(id) => self.ranks.get(id).copied().unwrap_or(Self::UNBOUND_RANK),
        }
    }
}

/// What one handler did with the value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step<V> {
    /// Returned `nil` (or failed): the value passes through unchanged.
    Pass,
    /// Returned a new value for the rest of the chain.
    Replace(V),
    /// Returned `{ cancel = true, message = … }`: stop the chain (cancellable hooks only).
    Cancel(Option<String>),
}

/// How a chain ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChainOutcome<V> {
    /// Every handler ran; the final value.
    Completed(V),
    /// A handler cancelled; later handlers did not run.
    Cancelled {
        /// The value when the chain stopped.
        value: V,
        /// The cancel message, if any.
        reason: Option<String>,
        /// Index (in chain order) of the handler that cancelled.
        by: usize,
    },
}

impl<V> ChainOutcome<V> {
    /// The value at the end of the chain (cancelled or not).
    #[must_use]
    pub fn value(&self) -> &V {
        match self {
            Self::Completed(v) | Self::Cancelled { value: v, .. } => v,
        }
    }

    /// Consumes the outcome, returning the final value.
    #[must_use]
    pub fn into_value(self) -> V {
        match self {
            Self::Completed(v) | Self::Cancelled { value: v, .. } => v,
        }
    }

    /// `true` if a handler cancelled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Cancelled { .. })
    }
}

/// Runs `handlers` (already in chain order) over `initial`, threading the value through
/// `call`. Stops at the first [`Step::Cancel`].
///
/// Failure policy is the caller's: a handler that errors should map to [`Step::Pass`]
/// (§9.6, the value passes through unchanged).
pub fn run_chain<H, V>(
    handlers: impl IntoIterator<Item = H>,
    initial: V,
    mut call: impl FnMut(H, &V) -> Step<V>,
) -> ChainOutcome<V> {
    let mut value = initial;
    for (index, handler) in handlers.into_iter().enumerate() {
        match call(handler, &value) {
            Step::Pass => {}
            Step::Replace(next) => value = next,
            Step::Cancel(reason) => {
                return ChainOutcome::Cancelled {
                    value,
                    reason,
                    by: index,
                };
            }
        }
    }
    ChainOutcome::Completed(value)
}

/// Handlers of one hook, kept sorted by [`ChainKey`].
#[derive(Clone, Debug)]
pub struct HookChain<H> {
    entries: Vec<(ChainKey, H)>,
}

impl<H> Default for HookChain<H> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<H> HookChain<H> {
    /// Empty chain.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Inserts `handler` at its place in chain order. Equal keys keep insertion order.
    pub fn insert(&mut self, key: ChainKey, handler: H) {
        let at = self.entries.partition_point(|(k, _)| *k <= key);
        self.entries.insert(at, (key, handler));
    }

    /// Number of handlers.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` when no handler is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `(key, handler)` pairs in chain order.
    pub fn iter(&self) -> impl Iterator<Item = (&ChainKey, &H)> {
        self.entries.iter().map(|(k, h)| (k, h))
    }

    /// Handlers in chain order, mutably.
    pub fn handlers_mut(&mut self) -> impl Iterator<Item = &mut H> {
        self.entries.iter_mut().map(|(_, h)| h)
    }

    /// Runs the chain over `initial` (see [`run_chain`]).
    pub fn run<V>(&self, initial: V, call: impl FnMut(&H, &V) -> Step<V>) -> ChainOutcome<V> {
        run_chain(self.entries.iter().map(|(_, h)| h), initial, call)
    }
}

/// Which registry a hook belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HookKind {
    /// `battle.on(name, fn, { priority })` (§7.4).
    Battle,
    /// `events.on(name, fn, { priority })` (§9.2).
    Event,
}

/// One handler as listed in a [`HookTable`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookTableEntry {
    /// Who registered it.
    pub owner: ScriptOwner,
    /// Its priority.
    pub priority: i64,
    /// `true` once disabled after repeated failures (§9.6).
    pub disabled: bool,
}

/// The resolved hook table: every hook name with its handlers in chain order (§9.4).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HookTable {
    /// Battle hooks, by hook name.
    pub battle: BTreeMap<String, Vec<HookTableEntry>>,
    /// World events, by event name.
    pub events: BTreeMap<String, Vec<HookTableEntry>>,
}

impl HookTable {
    /// Appends the handlers of `chain` (in chain order) under `kind` / `name`.
    pub fn add_chain<H>(
        &mut self,
        kind: HookKind,
        name: &str,
        chain: &HookChain<H>,
        entry: impl Fn(&ChainKey, &H) -> HookTableEntry,
    ) {
        if chain.is_empty() {
            return;
        }
        let map = match kind {
            HookKind::Battle => &mut self.battle,
            HookKind::Event => &mut self.events,
        };
        map.entry(name.to_owned())
            .or_default()
            .extend(chain.iter().map(|(k, h)| entry(k, h)));
    }

    /// `true` when no handler is registered at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.battle.is_empty() && self.events.is_empty()
    }
}

fn owner_label(owner: &ScriptOwner) -> String {
    match owner {
        ScriptOwner::Game => "game".to_owned(),
        ScriptOwner::Plugin(id) => id.clone(),
        ScriptOwner::Map(id) => format!("map:{id}"),
    }
}

/// Human-readable table, one hook per block:
///
/// ```text
/// battle hooks:
///   on_damage_taken
///     -5  shield
///      0  game
/// world events:
///   (none)
/// ```
impl fmt::Display for HookTable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (title, map) in [
            ("battle hooks", &self.battle),
            ("world events", &self.events),
        ] {
            writeln!(f, "{title}:")?;
            if map.is_empty() {
                writeln!(f, "  (none)")?;
            }
            for (name, entries) in map {
                writeln!(f, "  {name}")?;
                for e in entries {
                    write!(f, "    {:>4}  {}", e.priority, owner_label(&e.owner))?;
                    if e.disabled {
                        write!(f, " (disabled)")?;
                    }
                    writeln!(f)?;
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(priority: i64, rank: u32, seq: u64) -> ChainKey {
        ChainKey::new(priority, rank, seq)
    }

    #[test]
    fn orders_by_priority_then_load_order_then_registration() {
        let mut chain = HookChain::new();
        chain.insert(key(0, 1, 0), "b-first");
        chain.insert(key(5, 0, 1), "late");
        chain.insert(key(0, 0, 2), "a");
        chain.insert(key(-5, 2, 3), "early");
        chain.insert(key(0, 1, 4), "b-second");
        let order: Vec<&str> = chain.iter().map(|(_, h)| *h).collect();
        assert_eq!(order, ["early", "a", "b-first", "b-second", "late"]);
    }

    #[test]
    fn insertion_order_does_not_change_the_result() {
        let keys = [key(1, 0, 0), key(0, 2, 1), key(0, 1, 2), key(-3, 9, 3)];
        let mut forward = HookChain::new();
        for k in keys {
            forward.insert(k, k.seq);
        }
        let mut backward = HookChain::new();
        for k in keys.iter().rev() {
            backward.insert(*k, k.seq);
        }
        let a: Vec<u64> = forward.iter().map(|(_, h)| *h).collect();
        let b: Vec<u64> = backward.iter().map(|(_, h)| *h).collect();
        assert_eq!(a, b);
        assert_eq!(a, [3, 2, 1, 0]);
    }

    #[test]
    fn values_thread_through_the_chain() {
        let mut chain: HookChain<Box<dyn Fn(i64) -> i64>> = HookChain::new();
        chain.insert(key(10, 0, 0), Box::new(|v| v * 2));
        chain.insert(key(-10, 1, 1), Box::new(|v| v + 3));
        let out = chain.run(5, |f, v| Step::Replace(f(*v)));
        assert_eq!(out, ChainOutcome::Completed(16), "(5 + 3) * 2");
    }

    #[test]
    fn pass_leaves_the_value_unchanged() {
        let handlers = [None, Some(7), None];
        let mut seen = Vec::new();
        let out = run_chain(handlers, 1, |h, v| {
            seen.push(*v);
            h.map_or(Step::Pass, Step::Replace)
        });
        assert_eq!(seen, [1, 1, 7]);
        assert_eq!(out.into_value(), 7);
    }

    #[test]
    fn cancel_short_circuits() {
        let mut ran = Vec::new();
        let out = run_chain(0..5, 10, |i, v| {
            ran.push(i);
            match i {
                1 => Step::Replace(v + 1),
                2 => Step::Cancel(Some("text:blocked".into())),
                _ => Step::Replace(v * 100),
            }
        });
        assert_eq!(ran, [0, 1, 2], "handlers after the cancel never run");
        assert_eq!(
            out,
            ChainOutcome::Cancelled {
                value: 1001,
                reason: Some("text:blocked".into()),
                by: 2,
            }
        );
        assert!(out.is_cancelled());
        assert_eq!(*out.value(), 1001);
    }

    #[test]
    fn empty_chain_returns_the_initial_value() {
        let chain: HookChain<()> = HookChain::new();
        assert!(chain.is_empty());
        assert_eq!(
            chain.run("x", |(), _| Step::Pass),
            ChainOutcome::Completed("x")
        );
    }

    #[test]
    fn load_order_ranks_plugins_then_unbound_then_game() {
        let mut order = LoadOrder::new();
        assert_eq!(order.bind("base"), 0);
        assert_eq!(order.bind("addon"), 1);
        assert_eq!(order.bind("base"), 0, "rebinding keeps the first rank");
        assert_eq!(order.rank(&ScriptOwner::plugin("addon")), 1);
        assert_eq!(
            order.rank(&ScriptOwner::plugin("stray")),
            LoadOrder::UNBOUND_RANK
        );
        assert_eq!(order.rank(&ScriptOwner::Game), LoadOrder::GAME_RANK);
        assert_eq!(
            order.rank(&ScriptOwner::Map("town".into())),
            LoadOrder::GAME_RANK
        );
        assert!(order.rank(&ScriptOwner::plugin("addon")) < LoadOrder::UNBOUND_RANK);
    }

    #[test]
    fn hook_table_renders_in_chain_order() {
        let mut chain = HookChain::new();
        chain.insert(key(0, LoadOrder::GAME_RANK, 2), ScriptOwner::Game);
        chain.insert(key(-5, 1, 1), ScriptOwner::plugin("shield"));
        chain.insert(key(0, 0, 0), ScriptOwner::plugin("crit"));
        let mut table = HookTable::default();
        table.add_chain(HookKind::Battle, "on_damage_taken", &chain, |k, o| {
            HookTableEntry {
                owner: o.clone(),
                priority: k.priority,
                disabled: k.seq == 2,
            }
        });
        table.add_chain(
            HookKind::Event,
            "map_enter",
            &HookChain::<()>::new(),
            |_, ()| unreachable!("empty chains are skipped"),
        );
        assert_eq!(
            table.to_string(),
            "battle hooks:\n  on_damage_taken\n      -5  shield\n       0  crit\n       0  game (disabled)\nworld events:\n  (none)\n"
        );
    }
}
