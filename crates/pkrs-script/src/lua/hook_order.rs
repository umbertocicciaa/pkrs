//! Lua side of the hook-chain engine ([`crate::chain`], §9.4): plugin load ranks, the global
//! registration sequence, `{ priority = n }` parsing and the resolved hook table.
//!
//! Any registry that wants priority-ordered chains (`battle.on`, `events.on`) builds its keys
//! with [`chain_key`] and stores handlers in a [`HookChain`](crate::chain::HookChain).

use mlua::{Lua, Table};

use super::{LuaScriptHost, battle};
use crate::chain::{ChainKey, HookKind, HookTable, HookTableEntry, LoadOrder};
use crate::host::ScriptOwner;

/// App data: plugin load ranks and the next registration sequence number.
#[derive(Default)]
pub(super) struct HookOrder {
    load: LoadOrder,
    next_seq: u64,
}

/// Records that plugin `id` is next in load order (called by `bind_plugin`).
pub(super) fn bind_plugin_rank(lua: &Lua, id: &str) {
    if lua.app_data_ref::<HookOrder>().is_none() {
        lua.set_app_data(HookOrder::default());
    }
    if let Some(mut order) = lua.app_data_mut::<HookOrder>() {
        order.load.bind(id);
    }
}

/// `opts.priority` (default `0`) from an optional `{ priority = n }` table.
pub(super) fn parse_priority(opts: Option<&Table>) -> mlua::Result<i64> {
    match opts {
        Some(t) => Ok(t.raw_get::<Option<i64>>("priority")?.unwrap_or(0)),
        None => Ok(0),
    }
}

/// Chain key for a new handler of `owner` with `priority`; consumes one sequence number.
pub(super) fn chain_key(lua: &Lua, owner: &ScriptOwner, priority: i64) -> ChainKey {
    if lua.app_data_ref::<HookOrder>().is_none() {
        lua.set_app_data(HookOrder::default());
    }
    match lua.app_data_mut::<HookOrder>() {
        Some(mut order) => {
            let seq = order.next_seq;
            order.next_seq = order.next_seq.saturating_add(1);
            ChainKey::new(priority, order.load.rank(owner), seq)
        }
        None => ChainKey::new(priority, LoadOrder::new().rank(owner), 0),
    }
}

impl LuaScriptHost {
    /// The resolved hook table: every `battle.on` hook (and, once registered through
    /// `events.on`, every world event) with its handlers in chain order (§9.4). This is what
    /// `pkrs check --plugins` prints.
    #[must_use]
    pub fn hook_table(&self) -> HookTable {
        let mut table = HookTable::default();
        if let Some(reg) = self.lua.app_data_ref::<battle::Registry>() {
            for (name, chain) in reg.chains() {
                table.add_chain(HookKind::Battle, name, chain, |key, h| HookTableEntry {
                    owner: h.owner().clone(),
                    priority: key.priority,
                    disabled: h.is_disabled(),
                });
            }
        }
        table
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_follow_bound_load_order_and_registration() {
        let lua = Lua::new();
        bind_plugin_rank(&lua, "first");
        bind_plugin_rank(&lua, "second");
        let a = chain_key(&lua, &ScriptOwner::plugin("second"), 0);
        let b = chain_key(&lua, &ScriptOwner::plugin("first"), 0);
        let c = chain_key(&lua, &ScriptOwner::Game, -1);
        assert_eq!((a.rank, a.seq), (1, 0));
        assert_eq!((b.rank, b.seq), (0, 1));
        assert!(b < a, "load order beats registration order");
        assert!(c < b, "priority beats load order");
    }

    #[test]
    fn priority_defaults_to_zero() {
        let lua = Lua::new();
        assert_eq!(parse_priority(None).unwrap(), 0);
        let t: Table = lua.load("return { priority = -7 }").eval().unwrap();
        assert_eq!(parse_priority(Some(&t)).unwrap(), -7);
        let t: Table = lua.load("return {}").eval().unwrap();
        assert_eq!(parse_priority(Some(&t)).unwrap(), 0);
    }
}
