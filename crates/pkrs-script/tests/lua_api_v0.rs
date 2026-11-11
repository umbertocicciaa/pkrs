//! Lua API v0 bindings (§9.1–§9.3) against a fake [`ScriptWorld`]: argument validation, error
//! messages, yield behaviour, events, and the §9.3 `old_keeper` cutscene run unmodified.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use pkrs_audio::{AudioRequest, VolumeGroup};
use pkrs_core::{Direction, GridPos, Pcg64Rng};
use pkrs_data::{TextArgs, format_text};
use pkrs_render::UniformValue;
use pkrs_script::{
    ApiError, ApiResult, CameraCommand, CameraTarget, LuaScriptHost, MAX_PARTY, MemoryScripts,
    PartyCreature, Scheduler, ScriptEvent, ScriptHost, ScriptLimits, ScriptOwner, ScriptValue,
    ScriptWorld, TaskKind, TaskOutcome, TaskStatus, TileInfo, WaitCondition, WaitRequest,
    WakeSignal,
};

// ---------------------------------------------------------------------------------------------
// Fake world
// ---------------------------------------------------------------------------------------------

struct State {
    log: Vec<String>,
    flags: BTreeMap<String, bool>,
    vars: BTreeMap<String, ScriptValue>,
    map_vars: BTreeMap<String, ScriptValue>,
    bag: BTreeMap<String, u32>,
    party: Vec<PartyCreature>,
    npcs: BTreeMap<String, (GridPos, Direction)>,
    player: (GridPos, Direction),
    locked: bool,
    camera: Vec<CameraCommand>,
    audio: Vec<AudioRequest>,
    language: String,
}

#[derive(Clone)]
struct Fake(Rc<RefCell<State>>);

impl Fake {
    fn new() -> Self {
        Self(Rc::new(RefCell::new(State {
            log: Vec::new(),
            flags: BTreeMap::new(),
            vars: BTreeMap::new(),
            map_vars: BTreeMap::new(),
            bag: BTreeMap::new(),
            party: Vec::new(),
            npcs: BTreeMap::from([("keeper".to_owned(), (GridPos::new(5, 3), Direction::Down))]),
            player: (GridPos::new(5, 6), Direction::Up),
            locked: false,
            camera: Vec::new(),
            audio: Vec::new(),
            language: "en".to_owned(),
        })))
    }

    fn log(&self) -> Vec<String> {
        self.0.borrow().log.clone()
    }

    fn note(&self, entry: String) {
        self.0.borrow_mut().log.push(entry);
    }

    fn npc(&self, id: &str) -> ApiResult<(GridPos, Direction)> {
        self.0
            .borrow()
            .npcs
            .get(id)
            .copied()
            .ok_or_else(|| ApiError::new(format!("no npc `{id}` on this map")))
    }
}

impl ScriptWorld for Fake {
    fn flag(&self, name: &str) -> bool {
        self.0.borrow().flags.get(name).copied().unwrap_or(false)
    }
    fn set_flag(&mut self, name: &str, value: bool) {
        self.note(format!("set_flag {name}={value}"));
        self.0.borrow_mut().flags.insert(name.into(), value);
    }
    fn var(&self, name: &str) -> ScriptValue {
        self.0
            .borrow()
            .vars
            .get(name)
            .cloned()
            .unwrap_or(ScriptValue::Nil)
    }
    fn set_var(&mut self, name: &str, value: ScriptValue) {
        self.0.borrow_mut().vars.insert(name.into(), value);
    }
    fn give_item(&mut self, item: &str, count: u32) -> ApiResult<()> {
        if item == "missingno" {
            return Err(ApiError::new("unknown item `missingno`"));
        }
        self.note(format!("give_item {item}x{count}"));
        *self.0.borrow_mut().bag.entry(item.into()).or_default() += count;
        Ok(())
    }
    fn bag(&self) -> Vec<(String, u32)> {
        self.0
            .borrow()
            .bag
            .iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }
    fn money(&self) -> i64 {
        1500
    }
    fn give_creature(&mut self, species: &str, level: u8) -> ApiResult<()> {
        if species == "missingno" {
            return Err(ApiError::new("unknown species `missingno`"));
        }
        let mut state = self.0.borrow_mut();
        if state.party.len() >= MAX_PARTY {
            return Err(ApiError::new("the party is full"));
        }
        state.party.push(PartyCreature {
            species: species.into(),
            level,
        });
        Ok(())
    }
    fn party(&self) -> Vec<PartyCreature> {
        self.0.borrow().party.clone()
    }
    fn current_map(&self) -> Option<String> {
        Some("lighthouse_1f".into())
    }
    fn warp(&mut self, map: &str, to: GridPos, facing: Option<Direction>) -> ApiResult<()> {
        if map == "nowhere" {
            return Err(ApiError::new("no map `nowhere`"));
        }
        self.note(format!("warp {map} {to} {facing:?}"));
        Ok(())
    }
    fn tile(&self, pos: GridPos) -> Option<TileInfo> {
        (pos.x >= 0 && pos.y >= 0 && pos.x < 10 && pos.y < 10).then(|| TileInfo {
            solid: pos.x == 0,
            terrain: (pos.y == 9).then(|| "water".to_owned()),
        })
    }
    fn map_var(&self, name: &str) -> ScriptValue {
        self.0
            .borrow()
            .map_vars
            .get(name)
            .cloned()
            .unwrap_or(ScriptValue::Nil)
    }
    fn set_map_var(&mut self, name: &str, value: ScriptValue) {
        self.0.borrow_mut().map_vars.insert(name.into(), value);
    }
    fn npc_exists(&self, id: &str) -> bool {
        self.0.borrow().npcs.contains_key(id)
    }
    fn npc_pos(&self, id: &str) -> ApiResult<(GridPos, Direction)> {
        self.npc(id)
    }
    fn npc_face(&mut self, id: &str, dir: Direction) -> ApiResult<()> {
        let (pos, _) = self.npc(id)?;
        self.note(format!("npc_face {id} {dir}"));
        self.0.borrow_mut().npcs.insert(id.into(), (pos, dir));
        Ok(())
    }
    fn npc_move(&mut self, id: &str, path: &[Direction]) -> ApiResult<()> {
        self.npc(id)?;
        let path: Vec<String> = path.iter().map(ToString::to_string).collect();
        self.note(format!("npc_move {id} {}", path.join(",")));
        Ok(())
    }
    fn npc_set_sprite(&mut self, id: &str, sprite: &str) -> ApiResult<()> {
        self.npc(id)?;
        self.note(format!("npc_sprite {id} {sprite}"));
        Ok(())
    }
    fn npc_set_visible(&mut self, id: &str, visible: bool) -> ApiResult<()> {
        self.npc(id)?;
        self.note(format!("npc_visible {id} {visible}"));
        Ok(())
    }
    fn player_pos(&self) -> (GridPos, Direction) {
        self.0.borrow().player
    }
    fn player_face(&mut self, dir: Direction) {
        self.0.borrow_mut().player.1 = dir;
    }
    fn set_player_locked(&mut self, locked: bool) {
        self.0.borrow_mut().locked = locked;
    }
    fn set_field_state(&mut self, name: &str, value: bool) -> ApiResult<()> {
        if name != "surf" {
            return Err(ApiError::new(format!("unknown field state `{name}`")));
        }
        self.note(format!("field {name}={value}"));
        Ok(())
    }
    fn say(&mut self, text: &str, args: &TextArgs) -> ApiResult<()> {
        self.note(format!("say {}", format_text(text, args)));
        Ok(())
    }
    fn choose(
        &mut self,
        prompt: Option<&str>,
        args: &TextArgs,
        options: &[String],
    ) -> ApiResult<()> {
        let prompt = prompt.map(|p| format_text(p, args));
        self.note(format!("choose {prompt:?} {}", options.join("|")));
        Ok(())
    }
    fn notify(&mut self, text: &str, args: &TextArgs) {
        self.note(format!("notify {}", format_text(text, args)));
    }
    fn language(&self) -> String {
        self.0.borrow().language.clone()
    }
    fn languages(&self) -> Vec<String> {
        vec!["en".into(), "it".into()]
    }
    fn set_language(&mut self, language: &str) -> ApiResult<()> {
        if !self.languages().iter().any(|l| l == language) {
            return Err(ApiError::new(format!("unknown language `{language}`")));
        }
        self.0.borrow_mut().language = language.to_owned();
        Ok(())
    }
    fn text(&self, value: &str, args: &TextArgs) -> String {
        format!("[{}] {}", self.language(), format_text(value, args))
    }
    fn camera(&mut self, command: CameraCommand) -> ApiResult<()> {
        self.0.borrow_mut().camera.push(command);
        Ok(())
    }
    fn set_global_uniform(&mut self, name: &str, value: UniformValue) -> ApiResult<()> {
        self.note(format!("uniform {name} = {value:?}"));
        Ok(())
    }
    fn audio(&mut self, request: AudioRequest) -> ApiResult<()> {
        self.0.borrow_mut().audio.push(request);
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------------------------

fn bare_host() -> LuaScriptHost {
    LuaScriptHost::new(
        Box::new(MemoryScripts::new()),
        Box::new(Pcg64Rng::from_seed(28)),
        ScriptLimits::default(),
    )
    .unwrap()
}

fn host(fake: &Fake) -> LuaScriptHost {
    let mut h = bare_host();
    h.set_world(Box::new(fake.clone()));
    h
}

fn eval(h: &mut LuaScriptHost, src: &str) -> ScriptValue {
    h.run_source(&ScriptOwner::Game, "scripts/test.lua", src)
        .unwrap()
}

/// Runs `src` and returns the error text (`file:line: message`).
fn err(h: &mut LuaScriptHost, src: &str) -> String {
    h.run_source(&ScriptOwner::Game, "scripts/test.lua", src)
        .unwrap_err()
        .to_string()
}

fn s(v: &str) -> ScriptValue {
    ScriptValue::from(v)
}

/// Defines `body` as global `task_fn`, starts it, and returns its id and first status.
fn start(h: &mut LuaScriptHost, body: &str) -> (u32, TaskStatus) {
    eval(h, &format!("function task_fn(...)\n{body}\nend"));
    h.start_task(&ScriptOwner::Game, "task_fn", &[]).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Namespaces
// ---------------------------------------------------------------------------------------------

#[test]
fn every_api_v0_namespace_is_a_global_table() {
    let fake = Fake::new();
    let mut h = host(&fake);
    for ns in [
        "game", "map", "player", "ui", "camera", "time", "log", "events", "audio",
    ] {
        assert_eq!(
            eval(&mut h, &format!("return type({ns})")),
            s("table"),
            "{ns}"
        );
    }
    // Every owner gets its own copy (per-plugin `_ENV`, §9.6).
    h.run_source(&ScriptOwner::plugin("p"), "p.lua", "game = nil")
        .unwrap();
    assert_eq!(eval(&mut h, "return type(game)"), s("table"));
}

#[test]
fn bindings_without_a_world_fail_cleanly() {
    let mut h = bare_host();
    let e = err(&mut h, "return game.flag('x')");
    assert_eq!(
        e,
        "scripts/test.lua:1: game.flag: no game world is attached to the script host"
    );
    // Yielding calls report it before yielding.
    eval(&mut h, "function t() ui.say('hi') end");
    let e = h
        .start_task(&ScriptOwner::Game, "t", &[])
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        "scripts/test.lua:1: ui.say: no game world is attached to the script host"
    );
}

// ---------------------------------------------------------------------------------------------
// game
// ---------------------------------------------------------------------------------------------

#[test]
fn game_flags_and_vars() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(
        eval(&mut h, "return game.flag('met')"),
        ScriptValue::Bool(false)
    );
    eval(&mut h, "game.set_flag('met', true); game.set_flag('seen')");
    assert_eq!(
        eval(&mut h, "return game.flag('met') and game.flag('seen')"),
        ScriptValue::Bool(true)
    );
    eval(&mut h, "game.set_flag('met', false)");
    assert_eq!(
        eval(&mut h, "return game.flag('met')"),
        ScriptValue::Bool(false)
    );

    eval(
        &mut h,
        "game.set_var('steps', 12); game.set_var('t', { a = 1, 'x' })",
    );
    assert_eq!(
        eval(&mut h, "return game.var('steps')"),
        ScriptValue::Int(12)
    );
    assert_eq!(eval(&mut h, "return game.var('t').a"), ScriptValue::Int(1));
    assert_eq!(eval(&mut h, "return game.var('none')"), ScriptValue::Nil);
}

#[test]
fn game_argument_errors_name_the_function() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(
        err(&mut h, "game.flag(42)"),
        "scripts/test.lua:1: game.flag: flag name must be a string (got number)"
    );
    assert_eq!(
        err(&mut h, "game.flag('')"),
        "scripts/test.lua:1: game.flag: flag name must not be empty"
    );
    assert_eq!(
        err(&mut h, "\ngame.set_flag('x', 'yes')"),
        "scripts/test.lua:2: game.set_flag: value must be a boolean (got string)"
    );
    assert_eq!(
        err(&mut h, "game.set_var('f', print)"),
        "scripts/test.lua:1: game.set_var: value must be nil, a boolean, a number, a string or a table of those"
    );
}

#[test]
fn game_items_and_money() {
    let fake = Fake::new();
    let mut h = host(&fake);
    eval(
        &mut h,
        "game.give_item('lamp_oil'); game.give_item('potion', 3)",
    );
    assert_eq!(
        eval(
            &mut h,
            "local b = game.bag(); return b.lamp_oil * 10 + b.potion"
        ),
        ScriptValue::Int(13)
    );
    assert_eq!(eval(&mut h, "return game.money()"), ScriptValue::Int(1500));
    assert_eq!(
        err(&mut h, "game.give_item('potion', 0)"),
        "scripts/test.lua:1: game.give_item: count must be between 1 and 4294967295 (got 0)"
    );
    assert_eq!(
        err(&mut h, "game.give_item('missingno')"),
        "scripts/test.lua:1: game.give_item: unknown item `missingno`"
    );
}

#[test]
fn game_party_gives_and_lists_creatures() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(eval(&mut h, "return #game.party()"), ScriptValue::Int(0));
    eval(
        &mut h,
        "game.give_creature('leafkit'); game.give_creature('emberkit', 12)",
    );
    assert_eq!(
        eval(
            &mut h,
            "local p = game.party(); return p[1].species .. p[1].level .. p[2].species .. p[2].level"
        ),
        s("leafkit5emberkit12")
    );
    assert_eq!(
        err(&mut h, "game.give_creature('leafkit', 101)"),
        "scripts/test.lua:1: game.give_creature: level must be between 1 and 100 (got 101)"
    );
    assert_eq!(
        err(&mut h, "game.give_creature('missingno')"),
        "scripts/test.lua:1: game.give_creature: unknown species `missingno`"
    );
    eval(
        &mut h,
        "for _ = 1, 4 do game.give_creature('puddlefin', 3) end",
    );
    assert_eq!(
        err(&mut h, "game.give_creature('leafkit')"),
        "scripts/test.lua:1: game.give_creature: the party is full"
    );
}

// ---------------------------------------------------------------------------------------------
// map
// ---------------------------------------------------------------------------------------------

#[test]
fn map_queries_and_warp() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(eval(&mut h, "return map.current()"), s("lighthouse_1f"));
    assert_eq!(
        eval(
            &mut h,
            "local t = map.tile(0, 9); return tostring(t.solid) .. t.terrain"
        ),
        s("truewater")
    );
    assert_eq!(
        eval(&mut h, "return map.tile(3, 3).terrain"),
        ScriptValue::Nil
    );
    assert_eq!(eval(&mut h, "return map.tile(-1, 0)"), ScriptValue::Nil);
    eval(
        &mut h,
        "map.warp('route_1', 4, 5, 'left'); map.warp('house', 1, 2)",
    );
    assert_eq!(
        fake.log(),
        ["warp route_1 (4, 5) Some(Left)", "warp house (1, 2) None"]
    );
    assert_eq!(
        err(&mut h, "map.warp('route_1', 4, 5, 'north')"),
        "scripts/test.lua:1: map.warp: facing must be \"up\", \"down\", \"left\" or \"right\" (got \"north\")"
    );
    assert_eq!(
        err(&mut h, "map.warp('route_1', 4.5, 5)"),
        "scripts/test.lua:1: map.warp: x must be a whole number (got number)"
    );
    assert_eq!(
        err(&mut h, "map.warp('nowhere', 0, 0)"),
        "scripts/test.lua:1: map.warp: no map `nowhere`"
    );
    eval(&mut h, "map.set_var('doors', 2)");
    assert_eq!(eval(&mut h, "return map.var('doors')"), ScriptValue::Int(2));
}

// ---------------------------------------------------------------------------------------------
// npc
// ---------------------------------------------------------------------------------------------

#[test]
fn npc_objects_wrap_map_npcs() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(eval(&mut h, "return map.npc('ghost')"), ScriptValue::Nil);
    assert_eq!(
        eval(
            &mut h,
            "local n = map.npc('keeper'); return n.id .. ' ' .. tostring(n)"
        ),
        s("keeper npc(keeper)")
    );
    // Crossing into Rust, an npc object becomes a handle.
    assert_eq!(
        eval(&mut h, "return map.npc('keeper')"),
        ScriptValue::Npc("keeper".into())
    );
    assert_eq!(
        eval(
            &mut h,
            "local n = map.npc('keeper'); local x, y, f = n:pos(); return x .. ',' .. y .. ',' .. f"
        ),
        s("5,3,down")
    );
    eval(
        &mut h,
        "local n = map.npc('keeper'); n:face('left'); n:set_sprite('keeper_sad'); n:hide(); n:show()",
    );
    assert_eq!(
        fake.log(),
        [
            "npc_face keeper left",
            "npc_sprite keeper keeper_sad",
            "npc_visible keeper false",
            "npc_visible keeper true"
        ]
    );
    // Player at (5, 6) is below the keeper at (5, 3).
    eval(&mut h, "map.npc('keeper'):face_player()");
    assert_eq!(fake.log().last().unwrap(), "npc_face keeper down");
    // The metatable is sealed.
    assert_eq!(
        eval(&mut h, "return getmetatable(map.npc('keeper'))"),
        ScriptValue::Bool(false)
    );
}

#[test]
fn npc_argument_errors() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(
        err(&mut h, "map.npc('keeper').face('up')"),
        "scripts/test.lua:1: npc:face: expected an npc object as `self` (got string); call it with ':'"
    );
    assert_eq!(
        err(&mut h, "map.npc('keeper'):face('sideways')"),
        "scripts/test.lua:1: npc:face: direction must be \"up\", \"down\", \"left\" or \"right\" (got \"sideways\")"
    );
    // A handle whose NPC is gone reports the world's error.
    fake.0.borrow_mut().npcs.clear();
    let mut h2 = host(&fake);
    h2.run_source(
        &ScriptOwner::Game,
        "a.lua",
        "function f(n) n:face('up') end",
    )
    .unwrap();
    let e = h2
        .call_function(
            &ScriptOwner::Game,
            "f",
            &[ScriptValue::Npc("keeper".into())],
        )
        .unwrap_err()
        .to_string();
    assert_eq!(e, "a.lua:1: npc:face: no npc `keeper` on this map");
}

#[test]
fn npc_move_yields_until_the_path_finishes() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let (id, status) = start(
        &mut h,
        "local n = map.npc('keeper'); n:move({ 'up', 'up', 'left' }); return 'arrived'",
    );
    assert_eq!(
        status,
        TaskStatus::Waiting(WaitRequest::PathFinished {
            npc: "keeper".into()
        })
    );
    assert_eq!(fake.log(), ["npc_move keeper up,up,left"]);
    assert_eq!(
        h.resume_task(id, ScriptValue::Nil).unwrap(),
        TaskStatus::Finished(s("arrived"))
    );
}

#[test]
fn npc_move_validates_before_starting_the_walk() {
    let fake = Fake::new();
    let mut h = host(&fake);
    // Outside a task: error, and the NPC never starts walking.
    assert_eq!(
        err(&mut h, "local _ = map.npc('keeper'):move({ 'up' })"),
        "scripts/test.lua:1: npc:move must be called from a script task (coroutine)"
    );
    assert_eq!(
        err(&mut h, "local _ = map.npc('keeper'):move({ 'up', 'jump' })"),
        "scripts/test.lua:1: npc:move: path[2] must be \"up\", \"down\", \"left\" or \"right\" (got \"jump\")"
    );
    assert_eq!(
        err(&mut h, "local _ = map.npc('keeper'):move('up')"),
        "scripts/test.lua:1: npc:move: path must be a list of directions (got string)"
    );
    assert!(fake.log().is_empty());
    // An empty path finishes without waiting.
    let (_, status) = start(&mut h, "map.npc('keeper'):move({}); return 'still'");
    assert_eq!(status, TaskStatus::Finished(s("still")));
}

// ---------------------------------------------------------------------------------------------
// player
// ---------------------------------------------------------------------------------------------

#[test]
fn player_position_facing_lock_and_field_state() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(
        eval(
            &mut h,
            "local x, y, f = player.pos(); return x .. ',' .. y .. ',' .. f"
        ),
        s("5,6,up")
    );
    eval(&mut h, "player.face('right'); player.lock()");
    assert_eq!(fake.0.borrow().player.1, Direction::Right);
    assert!(fake.0.borrow().locked);
    eval(
        &mut h,
        "player.unlock(); player.set_field_state('surf', true)",
    );
    assert!(!fake.0.borrow().locked);
    assert_eq!(fake.log(), ["field surf=true"]);
    assert_eq!(
        err(&mut h, "player.set_field_state('fly', true)"),
        "scripts/test.lua:1: player.set_field_state: unknown field state `fly`"
    );
    assert_eq!(
        err(&mut h, "player.set_field_state('surf', 1)"),
        "scripts/test.lua:1: player.set_field_state: value must be a boolean (got number)"
    );
}

// ---------------------------------------------------------------------------------------------
// ui
// ---------------------------------------------------------------------------------------------

#[test]
fn ui_say_yields_until_the_dialogue_closes() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let (id, status) = start(
        &mut h,
        "ui.say('text:hello'); ui.say('text:bye'); return 'done'",
    );
    assert_eq!(status, TaskStatus::Waiting(WaitRequest::DialogueClosed));
    assert_eq!(fake.log(), ["say text:hello"]);
    assert_eq!(
        h.resume_task(id, ScriptValue::Nil).unwrap(),
        TaskStatus::Waiting(WaitRequest::DialogueClosed)
    );
    assert_eq!(
        h.resume_task(id, ScriptValue::Nil).unwrap(),
        TaskStatus::Finished(s("done"))
    );
    assert_eq!(fake.log(), ["say text:hello", "say text:bye"]);
}

#[test]
fn ui_choose_and_yes_no_return_the_answer() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let (id, _) = start(&mut h, "return ui.choose({ 'Red', 'Blue', 'Green' })");
    assert_eq!(fake.log(), ["choose None Red|Blue|Green"]);
    assert_eq!(
        h.resume_task(id, ScriptValue::Int(2)).unwrap(),
        TaskStatus::Finished(ScriptValue::Int(2))
    );
    let (id, _) = start(&mut h, "return ui.choose({ 'a' })");
    assert_eq!(
        h.resume_task(id, ScriptValue::Nil).unwrap(),
        TaskStatus::Finished(ScriptValue::Nil),
        "cancel gives nil"
    );
    for (answer, expected) in [(ScriptValue::Int(1), true), (ScriptValue::Int(2), false)] {
        let (id, status) = start(&mut h, "return ui.yes_no('text:sure')");
        assert_eq!(status, TaskStatus::Waiting(WaitRequest::DialogueClosed));
        assert_eq!(
            h.resume_task(id, answer).unwrap(),
            TaskStatus::Finished(ScriptValue::Bool(expected))
        );
    }
    assert_eq!(
        fake.log().last().unwrap(),
        "choose Some(\"text:sure\") text:ui.yes|text:ui.no"
    );
}

#[test]
fn ui_texts_take_arguments_and_game_switches_language() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let (id, _) = start(
        &mut h,
        "ui.say('Hi {name}, {0} and {1}!', { 'A', 2, name = 'Ash' })",
    );
    h.resume_task(id, ScriptValue::Nil).unwrap();
    let (_, _) = start(&mut h, "return ui.yes_no('Go {0}?', { 'north' })");
    eval(&mut h, "ui.notify('{n} saved', { n = 3.0 })");
    assert_eq!(
        fake.log(),
        [
            "say Hi Ash, A and 2!",
            "choose Some(\"Go north?\") text:ui.yes|text:ui.no",
            "notify 3 saved",
        ]
    );
    assert_eq!(eval(&mut h, "return game.language()"), s("en"));
    eval(&mut h, "game.set_language('it')");
    assert_eq!(eval(&mut h, "return game.language()"), s("it"));
    assert_eq!(
        eval(&mut h, "return game.text('text:x {v}', { v = true })"),
        s("[it] text:x true")
    );
    assert_eq!(
        eval(&mut h, "return table.concat(game.languages(), ',')"),
        s("en,it")
    );
    assert_eq!(
        err(&mut h, "game.set_language('fr')"),
        "scripts/test.lua:1: game.set_language: unknown language `fr`"
    );
    assert_eq!(
        err(&mut h, "game.text('x', 5)"),
        "scripts/test.lua:1: game.text: args must be a table (got number)"
    );
    assert_eq!(
        err(&mut h, "game.text('x', { f = print })"),
        "scripts/test.lua:1: game.text: args.f must be a string, number or boolean (got function)"
    );
}

#[test]
fn ui_argument_errors_point_at_the_caller() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(
        err(&mut h, "local _ = ui.say('hi')"),
        "scripts/test.lua:1: ui.say must be called from a script task (coroutine)"
    );
    assert_eq!(
        err(&mut h, "local _ = ui.yes_no('ok?')"),
        "scripts/test.lua:1: ui.yes_no must be called from a script task (coroutine)"
    );
    eval(
        &mut h,
        "function bad_say() ui.say(nil) end\nfunction bad_choose()\n  ui.choose({})\nend\nfunction mixed() ui.choose({ 'a', 7 }) end",
    );
    let e = h
        .start_task(&ScriptOwner::Game, "bad_say", &[])
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        "scripts/test.lua:1: ui.say: text must be a string (got nil)"
    );
    let e = h
        .start_task(&ScriptOwner::Game, "bad_choose", &[])
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        "scripts/test.lua:3: ui.choose: options must not be empty"
    );
    let e = h
        .start_task(&ScriptOwner::Game, "mixed", &[])
        .unwrap_err()
        .to_string();
    assert_eq!(
        e,
        "scripts/test.lua:5: ui.choose: option 2 must be a string (got number)"
    );
    assert!(fake.log().is_empty(), "nothing opened: {:?}", fake.log());
    eval(&mut h, "ui.notify('Saved!')");
    assert_eq!(fake.log(), ["notify Saved!"]);
}

// ---------------------------------------------------------------------------------------------
// camera
// ---------------------------------------------------------------------------------------------

#[test]
fn audio_requests_reach_the_world() {
    let fake = Fake::new();
    let mut h = host(&fake);
    eval(
        &mut h,
        "audio.play_bgm('town'); audio.play_bgm('route1', { fade = 12 }); audio.play_bgm('cave', 5); \
         audio.stop_bgm(); audio.stop_bgm({ fade = 0 }); audio.play_sfx('bump'); \
         audio.play_jingle('item_get'); audio.jingle('level_up'); audio.set_volume('bgm', 0.5)",
    );
    let bgm = |id: &str, fade| AudioRequest::PlayBgm {
        id: id.into(),
        fade,
    };
    assert_eq!(
        fake.0.borrow().audio,
        [
            bgm("town", None),
            bgm("route1", Some(12)),
            bgm("cave", Some(5)),
            AudioRequest::StopBgm { fade: None },
            AudioRequest::StopBgm { fade: Some(0) },
            AudioRequest::PlaySfx { id: "bump".into() },
            AudioRequest::PlayJingle {
                id: "item_get".into()
            },
            AudioRequest::PlayJingle {
                id: "level_up".into()
            },
            AudioRequest::SetVolume {
                group: VolumeGroup::Bgm,
                volume: 0.5
            },
        ]
    );
    assert_eq!(
        err(&mut h, "audio.play_bgm('')"),
        "scripts/test.lua:1: audio.play_bgm: id must not be empty"
    );
    assert_eq!(
        err(&mut h, "audio.play_sfx(3)"),
        "scripts/test.lua:1: audio.play_sfx: id must be a string (got number)"
    );
    assert_eq!(
        err(&mut h, "audio.set_volume('music', 1)"),
        "scripts/test.lua:1: audio.set_volume: group must be \"master\", \"bgm\", \"sfx\" or \"jingle\" (got \"music\")"
    );
    assert_eq!(
        err(&mut h, "audio.set_volume('sfx', 2)"),
        "scripts/test.lua:1: audio.set_volume: volume must be between 0 and 1 (got 2)"
    );
    assert_eq!(
        err(&mut h, "audio.stop_bgm('slow')"),
        "scripts/test.lua:1: audio.stop_bgm: options must be nil, a tick count or { fade = ticks } (got string)"
    );
}

#[test]
fn camera_commands_reach_the_world() {
    let fake = Fake::new();
    let mut h = host(&fake);
    eval(
        &mut h,
        "camera.pan_to(3, 4, 30); camera.follow(map.npc('keeper')); camera.follow('rival'); camera.follow(); camera.shake(); camera.fade_out(10); camera.fade_in()",
    );
    assert_eq!(
        fake.0.borrow().camera,
        [
            CameraCommand::PanTo {
                to: GridPos::new(3, 4),
                ticks: 30
            },
            CameraCommand::Follow(CameraTarget::Npc("keeper".into())),
            CameraCommand::Follow(CameraTarget::Npc("rival".into())),
            CameraCommand::Follow(CameraTarget::Player),
            CameraCommand::Shake {
                intensity: 2,
                ticks: 20
            },
            CameraCommand::FadeOut { ticks: 10 },
            CameraCommand::FadeIn { ticks: 20 },
        ]
    );
    assert_eq!(
        err(&mut h, "camera.shake(100)"),
        "scripts/test.lua:1: camera.shake: intensity must be between 0 and 64 (got 100)"
    );
    assert_eq!(
        err(&mut h, "camera.fade_out(-1)"),
        "scripts/test.lua:1: camera.fade_out: ticks must be between 0 and 4294967295 (got -1)"
    );
    assert_eq!(
        err(&mut h, "camera.follow(true)"),
        "scripts/test.lua:1: camera.follow: target must be \"player\", an npc object or an npc id (got boolean)"
    );
}

// ---------------------------------------------------------------------------------------------
// time
// ---------------------------------------------------------------------------------------------

#[test]
fn wait_yields_ticks_and_needs_a_task() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let (_, status) = start(&mut h, "wait(20)");
    assert_eq!(status, TaskStatus::Waiting(WaitRequest::Ticks(20)));
    assert_eq!(
        err(&mut h, "local _ = wait(1)"),
        "scripts/test.lua:1: time.wait must be called from a script task (coroutine)"
    );
}

// ---------------------------------------------------------------------------------------------
// events
// ---------------------------------------------------------------------------------------------

#[test]
fn events_dispatch_in_emission_then_registration_order() {
    let fake = Fake::new();
    let mut h = host(&fake);
    eval(
        &mut h,
        r#"
seen = {}
local function note(s) seen[#seen + 1] = s end
events.on("a", function(p) note("a1:" .. tostring(p and p.n)) end)
events.on("b", function() note("b1") end, { priority = 5 })
events.on("a", function() note("a2"); events.emit("c") end)
events.on("c", function() note("c1") end)
events.emit("b")
events.emit("a", { n = 7 })
function seen_log() return table.concat(seen, ",") end
"#,
    );
    h.emit_event(ScriptEvent::new("a", ScriptValue::Nil));
    let mut scheduler = Scheduler::new();
    let report = scheduler.dispatch_events(&mut h);
    // `c` is emitted by a listener, so it runs after the events already queued.
    assert_eq!(report.events, ["b", "a", "a", "c", "c"]);
    assert_eq!(report.started.len(), 7);
    assert!(report.failures.is_empty());
    assert_eq!(
        h.call_function(&ScriptOwner::Game, "seen_log", &[])
            .unwrap(),
        s("b1,a1:7,a2,a1:nil,a2,c1,c1")
    );
}

#[test]
fn events_on_validates_its_arguments() {
    let fake = Fake::new();
    let mut h = host(&fake);
    assert_eq!(
        err(&mut h, "events.on('', function() end)"),
        "scripts/test.lua:1: events.on: event name must not be empty"
    );
    assert_eq!(
        err(&mut h, "events.on('step', 'not a function')"),
        "scripts/test.lua:1: events.on: handler must be a function (got string)"
    );
    assert_eq!(
        err(&mut h, "events.on('step', function() end, 3)"),
        "scripts/test.lua:1: events.on: options must be a table like { priority = 0 } (got number)"
    );
    assert_eq!(
        err(&mut h, "events.emit(nil)"),
        "scripts/test.lua:1: events.emit: event name must be a string (got nil)"
    );
    // `off` removes a listener.
    eval(
        &mut h,
        "hits = 0; local id = events.on('x', function() hits = hits + 1 end); assert(events.off(id)); assert(not events.off(id))",
    );
    h.emit_event(ScriptEvent::new("x", ScriptValue::Nil));
    Scheduler::new().dispatch_events(&mut h);
    assert_eq!(eval(&mut h, "return hits"), ScriptValue::Int(0));
}

#[test]
fn listeners_run_as_tasks_and_interact_locks_the_player() {
    let fake = Fake::new();
    let mut h = host(&fake);
    eval(
        &mut h,
        r#"
events.on("interact", function(e)
  e.npc:face_player()
  ui.say("text:hi")
end)
events.on("map_enter", function(e) log.info("entered " .. e.map) end)
events.on("interact", function() error("listener bug") end)
"#,
    );
    h.emit_event(ScriptEvent::interact_npc("keeper", GridPos::new(5, 3)));
    h.emit_event(ScriptEvent::map_enter("lighthouse_1f"));
    let mut scheduler = Scheduler::new();
    let report = scheduler.dispatch_events(&mut h);
    assert_eq!(report.failures.len(), 1, "the failing listener is isolated");
    assert_eq!(report.started.len(), 2);
    assert!(matches!(
        report.started[0].1,
        TaskOutcome::Waiting(WaitCondition::DialogueClosed)
    ));
    assert!(matches!(report.started[1].1, TaskOutcome::Finished(_)));
    assert!(
        scheduler.player_locked(),
        "interact listeners lock the player"
    );
    assert_eq!(fake.log(), ["npc_face keeper down", "say text:hi"]);
    scheduler.signal(WakeSignal::DialogueClosed(ScriptValue::Nil));
    scheduler.step(&mut h);
    assert!(!scheduler.player_locked());
    assert!(scheduler.is_empty());
}

#[test]
fn unloading_an_owner_drops_its_listeners_and_globals() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let map = ScriptOwner::Map("route_1".into());
    h.run_source(
        &map,
        "scripts/maps/route_1.lua",
        "hits = 0\nevents.on('step', function() hits = hits + 1 end)\nfunction old_man() end",
    )
    .unwrap();
    eval(
        &mut h,
        "game_hits = 0; events.on('step', function() game_hits = game_hits + 1 end)",
    );
    assert!(h.is_loaded(&map));
    assert!(h.has_function(&map, "old_man"));
    assert_eq!(h.unload_owner(&map), 1);
    assert!(!h.is_loaded(&map));
    assert!(!h.has_function(&map, "old_man"));
    h.emit_event(ScriptEvent::step(GridPos::new(1, 1), Direction::Up));
    Scheduler::new().dispatch_events(&mut h);
    assert_eq!(eval(&mut h, "return game_hits"), ScriptValue::Int(1));
}

// ---------------------------------------------------------------------------------------------
// Acceptance: §9.3 `old_keeper`, unmodified
// ---------------------------------------------------------------------------------------------

/// The §9.3 example, verbatim.
const OLD_KEEPER: &str = r#"-- scripts/maps/lighthouse_1f.lua
function old_keeper(npc)
  npc:face_player()
  if not game.flag("keeper_met") then
    ui.say("text:keeper.hello")
    local yes = ui.yes_no("text:keeper.help_me")
    if yes then
      npc:move({ "up", "up", "left" })
      wait(20)
      ui.say("text:keeper.thanks")
      game.give_item("lamp_oil", 1)
      game.set_flag("keeper_met", true)
    end
  else
    ui.say("text:keeper.again")
  end
end
"#;

#[test]
fn old_keeper_runs_unmodified() {
    let fake = Fake::new();
    let mut h = host(&fake);
    let owner = ScriptOwner::Map("lighthouse_1f".into());
    h.run_source(&owner, "scripts/maps/lighthouse_1f.lua", OLD_KEEPER)
        .unwrap();
    let mut scheduler = Scheduler::new();
    let npc = [ScriptValue::Npc("keeper".into())];
    let (task, outcome) = scheduler
        .spawn(&mut h, &owner, "old_keeper", &npc, TaskKind::Interact)
        .unwrap();
    assert!(matches!(
        outcome,
        TaskOutcome::Waiting(WaitCondition::DialogueClosed)
    ));
    assert!(scheduler.player_locked());

    // hello → closed; yes/no → "Yes"; path → finished; wait(20); thanks → closed.
    scheduler.signal(WakeSignal::DialogueClosed(ScriptValue::Nil));
    scheduler.step(&mut h);
    assert_eq!(
        scheduler.wait_of(task),
        Some(&WaitCondition::DialogueClosed)
    );
    scheduler.signal(WakeSignal::DialogueClosed(ScriptValue::Int(1)));
    scheduler.step(&mut h);
    assert_eq!(
        scheduler.wait_of(task),
        Some(&WaitCondition::PathFinished {
            npc: "keeper".into()
        })
    );
    scheduler.signal(WakeSignal::PathFinished {
        npc: "keeper".into(),
    });
    let report = scheduler.step(&mut h);
    let WaitCondition::Ticks { resume_at } = scheduler.wait_of(task).cloned().unwrap() else {
        panic!("expected wait(20)");
    };
    assert_eq!(resume_at, report.tick + 20);
    while matches!(scheduler.wait_of(task), Some(WaitCondition::Ticks { .. })) {
        scheduler.step(&mut h);
    }
    assert_eq!(
        scheduler.wait_of(task),
        Some(&WaitCondition::DialogueClosed)
    );
    scheduler.signal(WakeSignal::DialogueClosed(ScriptValue::Nil));
    scheduler.step(&mut h);
    assert!(scheduler.is_empty());
    assert!(!scheduler.player_locked());
    assert_eq!(
        fake.log(),
        [
            "npc_face keeper down",
            "say text:keeper.hello",
            "choose Some(\"text:keeper.help_me\") text:ui.yes|text:ui.no",
            "npc_move keeper up,up,left",
            "say text:keeper.thanks",
            "give_item lamp_oilx1",
            "set_flag keeper_met=true",
        ]
    );

    // Talking again takes the `else` branch.
    scheduler
        .spawn(&mut h, &owner, "old_keeper", &npc, TaskKind::Interact)
        .unwrap();
    assert_eq!(fake.log().last().unwrap(), "say text:keeper.again");
}

// ---------------------------------------------------------------------------------------------
// render (§5.6)
// ---------------------------------------------------------------------------------------------

#[test]
fn render_global_uniforms_reach_the_world() {
    let fake = Fake::new();
    let mut h = host(&fake);
    eval(
        &mut h,
        "render.set_global_uniform('daynight:phase', 0.25); render.set_global_uniform('day_tint', { 1, 0.5, 0.25, 1 }); render.set_global_uniform('wind', { 2, 0 })",
    );
    assert_eq!(
        fake.log(),
        [
            "uniform daynight:phase = Float(0.25)",
            "uniform day_tint = Vec4([1.0, 0.5, 0.25, 1.0])",
            "uniform wind = Vec2([2.0, 0.0])",
        ]
    );
    assert_eq!(
        err(&mut h, "render.set_global_uniform('time_ticks', 1)"),
        "scripts/test.lua:1: render.set_global_uniform: `time_ticks` is an engine uniform and cannot be set"
    );
    assert_eq!(
        err(&mut h, "render.set_global_uniform('x', { 1 })"),
        "scripts/test.lua:1: render.set_global_uniform: value must be an array of 2-4 numbers (got 1 entries)"
    );
    assert_eq!(
        err(&mut h, "render.set_global_uniform('x', { 1, 'a' })"),
        "scripts/test.lua:1: render.set_global_uniform: value[2] must be a finite number (got string)"
    );
    assert_eq!(
        err(&mut h, "render.set_global_uniform('', 1)"),
        "scripts/test.lua:1: render.set_global_uniform: name must not be empty"
    );
    assert_eq!(
        err(&mut h, "render.set_global_uniform('x', true)"),
        "scripts/test.lua:1: render.set_global_uniform: value must be a number or an array of 2-4 numbers (got boolean)"
    );
    let mut bare = bare_host();
    assert_eq!(
        err(&mut bare, "render.set_global_uniform('x', 1)"),
        "scripts/test.lua:1: render.set_global_uniform: no game world is attached to the script host"
    );
}
