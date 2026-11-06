//! Lua trainer AI (§7.5 Lua AI, Plugin AI): [`LuaAi`] implements the battle crate's
//! [`AiScript`] port by calling a Lua `choose_action(view)`.
//!
//! - **Profile scripts:** [`LuaScriptHost::load_ai_script`] runs `AiProfile.script` in its own
//!   sandboxed environment (so two profile scripts never share globals) and keeps the
//!   `choose_action` it defines.
//! - **Plugin profiles:** `ai.register_profile(id, fn, opts?)` registers `fn` under `id`; a
//!   registered id replaces the behaviour of a data profile with the same id. `opts` may set
//!   `tier` (`"random"`, `"basic"`, `"smart"`: the fallback), `visibility` (`"fair"`, `"full"`),
//!   `allow_items` and `heal_below_percent`.
//! - **Calling convention:** `choose_action(view)` runs in a fresh coroutine under the usual
//!   instruction budget; it must not yield. `view` is a deep read-only table (writes raise an
//!   error) built from [`AiScriptView`]. It returns `nil` (let the built-in tier decide),
//!   `{ move = "ember" | slot, target = slot | creature }`, `{ switch = party_index | creature }`
//!   or `{ item = "potion", target = slot | creature }`. Slots and party indices are 0-based,
//!   like everywhere else in the battle API.
//! - **Determinism:** while the script runs, `math.random` draws from the AI's own stream
//!   (derived from the battle's `RngPolicy`), never from the shared script RNG, so the same
//!   seed gives the same decisions and recorded inputs replay without Lua.
//! - **Failures:** an error, a yield, a malformed result or an illegal choice falls back to the
//!   profile's tier (see `pkrs_battle::ScriptedAi`); after `max_failures` consecutive script
//!   failures the script is no longer called for that battle.

use std::collections::BTreeMap;

use mlua::{ChunkMode, Function, Lua, Table, ThreadStatus, Value};
use pkrs_battle::{
    AiChoice, AiCreatureInfo, AiMoveRef, AiRequest, AiScript, AiScriptView, AiSideInfo, RngPolicy,
    SideId, SlotRef,
};
use pkrs_core::{GameRng, Pcg64Rng};
use pkrs_data::{AiProfile, AiTier, AiVisibility, MoveCategory};

use super::{LuaScriptHost, engine, sandbox};
use crate::error::{Result, ScriptError};
use crate::host::ScriptOwner;

/// Label mixed into the AI seed for the Lua stream (distinct from the fallback tier's).
const LUA_AI_STREAM: u64 = 0x00A1_0C0A;

/// Default consecutive script failures before a [`LuaAi`] stops calling Lua.
pub const DEFAULT_AI_MAX_FAILURES: u32 = 3;

struct Entry {
    owner: String,
    func: Function,
}

/// App data: registered plugin profiles and loaded profile scripts.
#[derive(Default)]
pub(super) struct AiRegistry {
    profiles: BTreeMap<String, (Entry, AiProfile)>,
    scripts: BTreeMap<String, Entry>,
}

fn with_registry<R>(lua: &Lua, f: impl FnOnce(&mut AiRegistry) -> R) -> R {
    if lua.app_data_ref::<AiRegistry>().is_none() {
        lua.set_app_data(AiRegistry::default());
    }
    match lua.app_data_mut::<AiRegistry>() {
        Some(mut reg) => f(&mut reg),
        None => f(&mut AiRegistry::default()),
    }
}

fn parse_tier(name: &str) -> mlua::Result<AiTier> {
    match name {
        "random" => Ok(AiTier::Random),
        "basic" => Ok(AiTier::Basic),
        "smart" => Ok(AiTier::Smart),
        other => Err(mlua::Error::runtime(format!(
            "ai.register_profile: unknown tier '{other}' (random, basic, smart)"
        ))),
    }
}

fn parse_visibility(name: &str) -> mlua::Result<AiVisibility> {
    match name {
        "fair" => Ok(AiVisibility::Fair),
        "full" => Ok(AiVisibility::Full),
        other => Err(mlua::Error::runtime(format!(
            "ai.register_profile: unknown visibility '{other}' (fair, full)"
        ))),
    }
}

fn profile_from_opts(id: &str, opts: Option<&Table>) -> mlua::Result<AiProfile> {
    let mut profile = AiProfile::builtin(id, AiTier::Basic);
    let Some(opts) = opts else {
        return Ok(profile);
    };
    if let Some(tier) = opts.raw_get::<Option<String>>("tier")? {
        profile.tier = parse_tier(&tier)?;
    }
    if let Some(vis) = opts.raw_get::<Option<String>>("visibility")? {
        profile.visibility = parse_visibility(&vis)?;
    }
    if let Some(items) = opts.raw_get::<Option<bool>>("allow_items")? {
        profile.allow_items = items;
    }
    if let Some(pct) = opts.raw_get::<Option<u8>>("heal_below_percent")? {
        if pct > 100 {
            return Err(mlua::Error::runtime(
                "ai.register_profile: heal_below_percent must be <= 100",
            ));
        }
        profile.heal_below_percent = pct;
    }
    Ok(profile)
}

/// Installs the `ai` module into `env`.
pub(super) fn install(lua: &Lua, env: &Table, owner: &ScriptOwner) -> mlua::Result<()> {
    let ai = lua.create_table()?;
    let owner_name = owner.to_string();
    let register = lua.create_function(
        move |lua, (id, func, opts): (String, Function, Option<Table>)| {
            if id.is_empty() {
                return Err(mlua::Error::runtime(
                    "ai.register_profile: id must not be empty",
                ));
            }
            let profile = profile_from_opts(&id, opts.as_ref())?;
            let owner = owner_name.clone();
            with_registry(lua, |reg| {
                if let Some((old, _)) = reg.profiles.get(&id) {
                    tracing::info!(
                        target: "script",
                        profile = %id,
                        previous = %old.owner,
                        owner = %owner,
                        "AI profile replaced"
                    );
                }
                reg.profiles.insert(id, (Entry { owner, func }, profile));
            });
            Ok(())
        },
    )?;
    ai.raw_set("register_profile", register)?;
    env.raw_set("ai", ai)
}

impl LuaScriptHost {
    /// Runs the AI profile script at `path` (`AiProfile.script`) in its own sandboxed
    /// environment for `owner` and keeps the global `choose_action` it defines.
    pub fn load_ai_script(&mut self, owner: &ScriptOwner, path: &str) -> Result<()> {
        let source = {
            let files = self
                .lua
                .app_data_ref::<sandbox::Files>()
                .ok_or_else(|| ScriptError::Engine("script files unavailable".into()))?;
            files.0.read_script(path)?
        };
        let env = sandbox::build_env(&self.lua, owner, &self.guard).map_err(engine)?;
        let chunk_env = env.clone();
        self.protected(path, |lua| {
            lua.load(source.as_str())
                .set_name(format!("@{path}"))
                .set_mode(ChunkMode::Text)
                .set_environment(chunk_env)
                .exec()
        })?;
        let Value::Function(func) = env.raw_get::<Value>("choose_action").map_err(engine)? else {
            return Err(ScriptError::NoSuchFunction {
                owner: owner.to_string(),
                name: format!("choose_action (in {path})"),
            });
        };
        let owner = owner.to_string();
        with_registry(&self.lua, |reg| {
            reg.scripts.insert(path.to_owned(), Entry { owner, func });
        });
        Ok(())
    }

    /// Profile ids registered with `ai.register_profile`, sorted.
    #[must_use]
    pub fn ai_profile_ids(&self) -> Vec<String> {
        with_registry(&self.lua, |reg| reg.profiles.keys().cloned().collect())
    }

    /// The profile a plugin registered as `id` (tier, visibility and item rules from its
    /// `opts`), so trainers can reference plugin-only ids.
    #[must_use]
    pub fn registered_ai_profile(&self, id: &str) -> Option<AiProfile> {
        with_registry(&self.lua, |reg| {
            reg.profiles.get(id).map(|(_, p)| p.clone())
        })
    }

    /// The Lua AI for `profile` playing `side` of a battle created with `policy`: the function
    /// registered under `profile.id` if any (plugins may replace a data profile), else the
    /// loaded `profile.script`. `None` means the profile is purely built-in.
    #[must_use]
    pub fn lua_ai(
        &self,
        profile: &AiProfile,
        policy: &RngPolicy,
        side: SideId,
    ) -> Option<LuaAi<'_>> {
        let found = with_registry(&self.lua, |reg| {
            if let Some((e, _)) = reg.profiles.get(&profile.id) {
                return Some((e.func.clone(), e.owner.clone()));
            }
            let path = profile.script.as_ref()?;
            reg.scripts
                .get(path)
                .map(|e| (e.func.clone(), e.owner.clone()))
        })?;
        let seed = Pcg64Rng::from_seed(policy.ai_seed(side)).derive_sub_seed(LUA_AI_STREAM);
        Some(LuaAi {
            host: self,
            func: found.0,
            owner: found.1,
            name: profile.id.clone(),
            rng: Some(Box::new(Pcg64Rng::from_seed(seed))),
            failures: 0,
            max_failures: DEFAULT_AI_MAX_FAILURES,
        })
    }
}

/// [`AiScript`] backed by a Lua `choose_action(view)`; wrap it in `pkrs_battle::ScriptedAi`.
pub struct LuaAi<'h> {
    host: &'h LuaScriptHost,
    func: Function,
    owner: String,
    name: String,
    rng: Option<Box<dyn GameRng>>,
    failures: u32,
    max_failures: u32,
}

impl LuaAi<'_> {
    /// Consecutive failures before the script is no longer called (default
    /// [`DEFAULT_AI_MAX_FAILURES`]; `0` never gives up).
    #[must_use]
    pub fn with_max_failures(mut self, n: u32) -> Self {
        self.max_failures = n;
        self
    }

    /// Consecutive failures so far.
    #[must_use]
    pub fn failures(&self) -> u32 {
        self.failures
    }

    /// `true` once the script has been given up on.
    #[must_use]
    pub fn disabled(&self) -> bool {
        self.max_failures > 0 && self.failures >= self.max_failures
    }

    fn call(&mut self, view: &AiScriptView) -> Result<AiChoice> {
        let lua = &self.host.lua;
        // `math.random` draws from this AI's stream for the duration of the call.
        let ai_rng = self
            .rng
            .take()
            .unwrap_or_else(|| Box::new(Pcg64Rng::from_seed(0)));
        let shared = lua.set_app_data(sandbox::Rng(ai_rng));
        let func = self.func.clone();
        let result = self.host.call_protected("choose_action", |lua| {
            let table = view_table(lua, view)?;
            let frozen = freeze(lua, table)?;
            let thread = lua.create_thread(func)?;
            let ret: mlua::MultiValue = thread.resume(frozen)?;
            if thread.status() == ThreadStatus::Resumable {
                return Err(mlua::Error::runtime(
                    "choose_action must decide synchronously (it yielded)",
                ));
            }
            parse_choice(&ret.into_iter().next().unwrap_or(Value::Nil), view)
        });
        let ai_rng = match shared {
            Some(prev) => lua.set_app_data(prev),
            None => lua.remove_app_data::<sandbox::Rng>(),
        };
        self.rng = ai_rng.map(|r| r.0);
        result
    }
}

impl AiScript for LuaAi<'_> {
    fn choose(&mut self, view: &AiScriptView) -> std::result::Result<AiChoice, String> {
        if self.disabled() {
            return Err(format!(
                "Lua AI `{}` disabled after {} consecutive failures",
                self.name, self.failures
            ));
        }
        match self.call(view) {
            Ok(choice) => {
                self.failures = 0;
                Ok(choice)
            }
            Err(err) => {
                self.failures = self.failures.saturating_add(1);
                tracing::warn!(
                    target: "script",
                    profile = %self.name,
                    owner = %self.owner,
                    failures = self.failures,
                    "Lua AI failed; using the built-in tier: {err}"
                );
                Err(err.to_string())
            }
        }
    }
}

// --- view marshalling --------------------------------------------------------------------

/// Deep read-only proxy: reads, `#`, `pairs` and `ipairs` work; writes raise an error.
const FREEZE: &str = r#"
local setmetatable, pairs, type, error, next = setmetatable, pairs, type, error, next
local function freeze(t)
  for k, v in pairs(t) do
    if type(v) == "table" then t[k] = freeze(v) end
  end
  return setmetatable({}, {
    __index = t,
    __newindex = function() error("the AI view is read-only", 2) end,
    __len = function() return #t end,
    __pairs = function() return next, t, nil end,
    __metatable = false,
  })
end
return freeze
"#;

fn freeze(lua: &Lua, t: Table) -> mlua::Result<Table> {
    let f: Function = lua.load(FREEZE).set_name("=pkrs-ai").eval()?;
    f.call(t)
}

fn side_name(side: SideId) -> &'static str {
    match side {
        SideId::Player => "player",
        SideId::Opponent => "opponent",
    }
}

fn category_name(c: MoveCategory) -> &'static str {
    match c {
        MoveCategory::Physical => "physical",
        MoveCategory::Special => "special",
        MoveCategory::Status => "status",
    }
}

fn creature_table(lua: &Lua, side: SideId, c: &AiCreatureInfo) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("side", side_name(side))?;
    t.raw_set("slot", c.slot)?;
    t.raw_set("active", c.slot.is_some())?;
    t.raw_set("party_index", c.party_index)?;
    t.raw_set("species", c.species.as_str())?;
    t.raw_set("name", c.name.as_str())?;
    t.raw_set("level", c.level)?;
    t.raw_set("hp_percent", c.hp_percent)?;
    t.raw_set("hp", c.hp)?;
    t.raw_set("max_hp", c.max_hp)?;
    t.raw_set("fainted", c.fainted)?;
    let types = lua.create_table()?;
    for (i, ty) in c.types.iter().enumerate() {
        types.raw_set(i + 1, ty.as_str())?;
    }
    t.raw_set("types", types)?;
    if let Some(s) = &c.stats {
        let stats = lua.create_table()?;
        stats.raw_set("max_hp", s.max_hp)?;
        stats.raw_set("attack", s.attack)?;
        stats.raw_set("defense", s.defense)?;
        stats.raw_set("speed", s.speed)?;
        t.raw_set("stats", stats)?;
    }
    let stages = lua.create_table()?;
    stages.raw_set("attack", c.stages.attack)?;
    stages.raw_set("defense", c.stages.defense)?;
    stages.raw_set("speed", c.stages.speed)?;
    stages.raw_set("accuracy", c.stages.accuracy)?;
    stages.raw_set("evasion", c.stages.evasion)?;
    t.raw_set("stages", stages)?;
    let moves = lua.create_table()?;
    for (i, m) in c.moves.iter().enumerate() {
        let mt = lua.create_table()?;
        mt.raw_set("slot", m.slot)?;
        mt.raw_set("id", m.id.as_str())?;
        mt.raw_set("pp", m.pp)?;
        mt.raw_set("max_pp", m.max_pp)?;
        if let Some(def) = &m.def {
            mt.raw_set("type", def.type_id.as_str())?;
            mt.raw_set("category", category_name(def.category))?;
            mt.raw_set("power", def.power)?;
            mt.raw_set("accuracy", def.accuracy)?;
            mt.raw_set("priority", def.priority)?;
        }
        moves.raw_set(i + 1, mt)?;
    }
    t.raw_set("moves", moves)?;
    Ok(t)
}

fn side_table(lua: &Lua, s: &AiSideInfo) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("side", side_name(s.side))?;
    t.raw_set("name", s.name.as_str())?;
    t.raw_set("party_size", s.party_size)?;
    let party = lua.create_table()?;
    for (i, c) in s.party.iter().enumerate() {
        party.raw_set(i + 1, creature_table(lua, s.side, c)?)?;
    }
    t.raw_set("party", party)?;
    // `active[slot + 1]` is the creature in that slot (absent when empty or hidden).
    let active = lua.create_table()?;
    for (slot, idx) in s.active.iter().enumerate() {
        if let Some(c) = idx.and_then(|i| s.member(i)) {
            active.raw_set(slot + 1, creature_table(lua, s.side, c)?)?;
        }
    }
    t.raw_set("active", active)?;
    Ok(t)
}

/// The `view` table passed to `choose_action`.
fn view_table(lua: &Lua, v: &AiScriptView) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    let (request, slot) = match v.request {
        AiRequest::Action { slot } => ("action", slot),
        AiRequest::Replace { slot } => ("replace", slot),
    };
    t.raw_set("request", request)?;
    t.raw_set("slot", slot)?;
    t.raw_set("side", side_name(v.side))?;
    t.raw_set("turn", v.turn)?;
    t.raw_set("side_size", v.side_size)?;
    t.raw_set(
        "visibility",
        match v.visibility {
            AiVisibility::Fair => "fair",
            AiVisibility::Full => "full",
        },
    )?;
    let items = lua.create_table()?;
    for (i, item) in v.items.iter().enumerate() {
        items.raw_set(i + 1, item.as_str())?;
    }
    t.raw_set("items", items)?;
    if let Some(me) = v.me() {
        t.raw_set("me", creature_table(lua, v.side, me)?)?;
    }
    t.raw_set("own", side_table(lua, &v.own)?)?;
    t.raw_set("foe", side_table(lua, &v.foe)?)?;
    Ok(t)
}

// --- result parsing ----------------------------------------------------------------------

fn small_int(v: &Value, what: &str) -> mlua::Result<u8> {
    let n = match v {
        Value::Integer(n) => *n,
        Value::Number(n) if n.fract() == 0.0 && n.is_finite() => *n as i64,
        other => {
            return Err(mlua::Error::runtime(format!(
                "{what} must be an integer (got {})",
                other.type_name()
            )));
        }
    };
    u8::try_from(n).map_err(|_| mlua::Error::runtime(format!("{what} out of range ({n})")))
}

fn parse_side_name(v: &Value) -> mlua::Result<SideId> {
    let Value::String(s) = v else {
        return Err(mlua::Error::runtime("side must be 'player' or 'opponent'"));
    };
    match s.to_str()?.as_ref() {
        "player" => Ok(SideId::Player),
        "opponent" => Ok(SideId::Opponent),
        other => Err(mlua::Error::runtime(format!("unknown side '{other}'"))),
    }
}

/// A target: a slot on `default_side`, or a creature table (`side` + `slot`).
fn parse_target(v: &Value, default_side: SideId) -> mlua::Result<Option<SlotRef>> {
    match v {
        Value::Nil => Ok(None),
        Value::Table(t) => {
            let side = parse_side_name(&t.get::<Value>("side")?)?;
            let slot = t.get::<Value>("slot")?;
            if slot.is_nil() {
                return Err(mlua::Error::runtime("target creature is not active"));
            }
            Ok(Some(SlotRef::new(side, small_int(&slot, "target slot")?)))
        }
        other => Ok(Some(SlotRef::new(
            default_side,
            small_int(other, "target")?,
        ))),
    }
}

/// `choose_action`'s return value → [`AiChoice`] (shape only; legality is checked by the
/// battle crate).
fn parse_choice(v: &Value, view: &AiScriptView) -> mlua::Result<AiChoice> {
    let t = match v {
        Value::Nil => return Ok(AiChoice::Default),
        Value::Table(t) => t,
        other => {
            return Err(mlua::Error::runtime(format!(
                "choose_action must return nil or an action table (got {})",
                other.type_name()
            )));
        }
    };
    let mv = t.get::<Value>("move")?;
    let switch = t.get::<Value>("switch")?;
    let item = t.get::<Value>("item")?;
    let target = t.get::<Value>("target")?;
    match (mv.is_nil(), switch.is_nil(), item.is_nil()) {
        (false, true, true) => {
            let mv = match &mv {
                Value::String(s) => AiMoveRef::Id(s.to_str()?.to_owned()),
                other => AiMoveRef::Slot(small_int(other, "move slot")?),
            };
            let default_side = match t.get::<Value>("target_side")? {
                Value::Nil => view.side.other(),
                side => parse_side_name(&side)?,
            };
            Ok(AiChoice::Move {
                mv,
                target: parse_target(&target, default_side)?,
            })
        }
        (true, false, true) => {
            let to_party = match &switch {
                Value::Table(c) => small_int(&c.get::<Value>("party_index")?, "switch")?,
                other => small_int(other, "switch")?,
            };
            Ok(AiChoice::Switch { to_party })
        }
        (true, true, false) => {
            let Value::String(id) = &item else {
                return Err(mlua::Error::runtime("item must be an item id string"));
            };
            let target_slot = match parse_target(&target, view.side)? {
                Some(t) if t.side != view.side => {
                    return Err(mlua::Error::runtime("items can only target your own side"));
                }
                t => t.map(|t| t.slot),
            };
            Ok(AiChoice::Item {
                item_id: id.to_str()?.to_owned(),
                target_slot,
            })
        }
        _ => Err(mlua::Error::runtime(
            "choose_action must return exactly one of move, switch or item",
        )),
    }
}

#[cfg(test)]
mod tests;
