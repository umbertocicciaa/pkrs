//! Integration (§9.4, §6.3, §10): plugin content registration (`data.register_*`, data
//! folders), terrain rules, field, battle and input actions, and `plugin.save`.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;

use pkrs_battle::{
    Battle, BattleAction, BattleConfig, BattleEvent, BattleInput, RngPolicy, SideId, SideInput,
    SlotRef, battle_testkit,
};
use pkrs_core::{Direction, GridPos, Pcg64Rng};
use pkrs_data::{ContentKind, ContentSource};
use pkrs_script::{
    FieldActionSource, FieldContext, LuaScriptHost, MemoryScripts, ScriptHost, ScriptLimits,
    ScriptOwner, ScriptValue, TaskStatus, load_plugins, plan_plugins,
};
use pkrs_world::{FieldState, TerrainQuery, TerrainRules, TerrainStepVerdict};

fn manifest(id: &str, deps: &str) -> String {
    format!(
        "[plugin]\nid = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = 1\n[dependencies]\n{deps}"
    )
}

fn host(files: &MemoryScripts) -> LuaScriptHost {
    LuaScriptHost::new(
        Box::new(files.clone()),
        Box::new(Pcg64Rng::from_seed(3)),
        ScriptLimits::default(),
    )
    .unwrap()
}

fn load(files: &MemoryScripts, enabled: &[&str]) -> (LuaScriptHost, pkrs_script::PluginLoadReport) {
    let enabled: Vec<String> = enabled.iter().map(|s| (*s).to_owned()).collect();
    let plan = plan_plugins(files, &enabled, 1..=1).unwrap();
    let mut h = host(files);
    let report = load_plugins(&mut h, files, &plan);
    (h, report)
}

fn run(h: &mut LuaScriptHost, owner: &ScriptOwner, src: &str) -> Result<ScriptValue, String> {
    h.run_source(owner, "test.lua", src)
        .map_err(|e| e.to_string())
}

const MOVE: &str = "(id: \"ember\", type: \"fire\", category: special, power: 40, \
                    accuracy: 100, priority: 0, effects: [(effect: \"damage\")])";

#[test]
fn data_folders_merge_before_init_and_registrations_follow_the_override_rules() {
    let files = MemoryScripts::new()
        .with("data/moves/ember.ron", MOVE)
        .with("plugins/base/plugin.toml", manifest("base", ""))
        .with(
            "plugins/base/data/moves/ember.ron",
            MOVE.replace("power: 40", "power: 50, override: true"),
        )
        .with(
            "plugins/base/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            seen_ember = data.has("move", "ember")
            data.register_move {
              id = "base:mud_bomb", type = "ground", category = "special",
              power = 65, accuracy = 85, priority = 0,
              effects = { { effect = "damage" } },
            }
            data.register_item { id = "boots", price = 300 }
            data.register_ability { id = "base:sticky" }
            data.register_status { id = "base:soggy", kind = "volatile", volatile_turns = 3 }
            data.register_terrain { id = "mud", requires_field = "base:boots" }
            data.register_effect({ id = "base:splash" }, function(n) return n * 2 end)
            "#,
        )
        .with("plugins/hot/plugin.toml", manifest("hot", "base = \"1\"\n"))
        .with(
            "plugins/hot/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            ok_dup, dup_err = perr(data.register_item, { id = "boots" })
            data.register_item { id = "boots", price = 999, override = true }
            ok_ns, ns_err = perr(data.register_item, { id = "base:stolen" })
            ok_bad, bad_err = perr(data.register_move, { id = "x" })
            ok_builtin, builtin_err = perr(data.register_terrain, { id = "water" })
            ids = data.ids("item")
            "#,
        );
    let (mut h, report) = load(&files, &["hot", "base"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    assert!(report.content_errors.is_empty());
    assert_eq!(report.content_files, 2);
    assert_eq!(report.content_overrides.len(), 1);

    let base = ScriptOwner::plugin("base");
    let hot = ScriptOwner::plugin("hot");
    assert_eq!(h.global(&base, "seen_ember").unwrap(), true.into());
    let mut err = |name: &str| h.global(&hot, name).unwrap();
    assert!(
        err("dup_err")
            .as_str()
            .unwrap()
            .contains("duplicate item `boots`: already defined by plugin `base`"),
        "{:?}",
        err("dup_err")
    );
    assert!(
        err("ns_err")
            .as_str()
            .unwrap()
            .contains("`base:` namespace")
    );
    assert!(
        err("bad_err")
            .as_str()
            .unwrap()
            .contains("data.register_move")
    );
    assert!(
        err("builtin_err")
            .as_str()
            .unwrap()
            .contains("built-in terrain")
    );
    assert_eq!(err("ids"), ScriptValue::List(vec!["boots".into()]));

    h.with_content(|db| {
        assert_eq!(db.move_def("ember").unwrap().power, 50);
        assert_eq!(db.move_def("base:mud_bomb").unwrap().accuracy, 85);
        assert_eq!(db.item("boots").unwrap().price, 999);
        assert_eq!(
            db.get(ContentKind::Item, "boots").unwrap().source,
            ContentSource::Plugin("hot".into())
        );
        assert!(db.ability("base:sticky").is_some());
        assert_eq!(db.status("base:soggy").unwrap().volatile_turns, Some(3));
        assert!(db.effect("base:splash").is_some());
        assert_eq!(db.overrides().len(), 2, "folder + Lua override");
    });
    assert_eq!(h.effect_handlers(), ["base:splash"]);
    assert_eq!(
        h.run_effect("base:splash", &[ScriptValue::Int(21)])
            .unwrap(),
        ScriptValue::Int(42)
    );
    let _ = run(&mut h, &ScriptOwner::Game, "return 1").unwrap();
}

#[test]
fn duplicate_folder_content_is_reported() {
    let files = MemoryScripts::new()
        .with("data/moves/ember.ron", MOVE)
        .with("plugins/a/plugin.toml", manifest("a", ""))
        .with("plugins/a/data/moves/ember.ron", MOVE);
    let (_, report) = load(&files, &["a"]);
    assert_eq!(report.content_errors.len(), 1);
    assert!(
        report.content_errors[0]
            .to_string()
            .contains("must declare `override: true`")
    );
}

fn query<'a>(terrain: &'a str, field: &'a FieldState) -> TerrainQuery<'a> {
    TerrainQuery {
        terrain,
        field,
        from: GridPos::new(1, 1),
        to: GridPos::new(1, 0),
        dir: Direction::Up,
    }
}

#[test]
fn terrain_rules_from_lua_and_data() {
    let files = MemoryScripts::new()
        .with("plugins/mud/plugin.toml", manifest("mud", ""))
        .with(
            "plugins/mud/data/terrains/t.ron",
            "[(id: \"lava\", passable: false), (id: \"deep\", requires_field: Some(\"surf\"))]",
        )
        .with(
            "plugins/mud/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            world.register_terrain("mud", function(ctx)
              last = ctx
              return ctx.field["mud:boots"] == true
            end)
            map.register_terrain("mud:bog", function(ctx) return "block" end)
            map.register_terrain("broken", function(ctx) error("boom") end)
            ok_dup, dup_err = perr(world.register_terrain, "mud", function() end)
            world.register_terrain("broken", function() return true end, { override = true })
            ok_builtin, builtin_err = perr(world.register_terrain, "ice", function() end)
            ok_ns, ns_err = perr(world.register_terrain, "other:x", function() end)
            "#,
        );
    let (mut h, report) = load(&files, &["mud"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let owner = ScriptOwner::plugin("mud");
    for (name, needle) in [
        ("dup_err", "already has a rule from plugin `mud`"),
        ("builtin_err", "built-in terrain"),
        ("ns_err", "`other:` namespace"),
    ] {
        let msg = h.global(&owner, name).unwrap();
        assert!(msg.as_str().unwrap().contains(needle), "{msg:?}");
    }
    assert_eq!(h.terrain_rules(), ["broken", "mud", "mud:bog"]);

    let mut field = FieldState::default();
    assert_eq!(h.verdict(&query("mud", &field)), TerrainStepVerdict::Block);
    field.set("mud:boots", true);
    assert_eq!(h.verdict(&query("mud", &field)), TerrainStepVerdict::Allow);
    let ScriptValue::Map(ctx) = h.global(&owner, "last").unwrap() else {
        panic!("ctx")
    };
    assert_eq!(ctx["dir"], "up".into());
    assert_eq!(ctx["y"], ScriptValue::Int(0));
    assert_eq!(
        h.verdict(&query("mud:bog", &field)),
        TerrainStepVerdict::Block
    );
    assert_eq!(
        h.verdict(&query("broken", &field)),
        TerrainStepVerdict::Allow
    );
    // Data terrains.
    assert_eq!(h.verdict(&query("lava", &field)), TerrainStepVerdict::Block);
    assert_eq!(h.verdict(&query("deep", &field)), TerrainStepVerdict::Block);
    field.set("surf", true);
    assert_eq!(h.verdict(&query("deep", &field)), TerrainStepVerdict::Allow);
    assert_eq!(
        h.verdict(&query("unknown", &field)),
        TerrainStepVerdict::Allow
    );
    let _ = run(&mut h, &owner, "return 1");
}

fn ctx(source: FieldActionSource, terrain: Option<&str>) -> FieldContext {
    FieldContext {
        source,
        player: GridPos::new(2, 2),
        facing: Direction::Left,
        target_terrain: terrain.map(str::to_owned),
    }
}

#[test]
fn field_actions_from_the_party_menu_and_facing_a_tile() {
    let files = MemoryScripts::new()
        .with("plugins/tools/plugin.toml", manifest("tools", ""))
        .with(
            "plugins/tools/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            world.register_field_action("tools:cut", {
              label = "text:tools.cut", terrain = "small_tree",
              run = function(ctx) used = ctx.source .. "@" .. ctx.target.x .. "," .. ctx.target.y end,
            })
            world.register_field_action("tools:flash", {
              available = function(ctx) return flash_ok end,
              run = function(ctx) ui_wait = coroutine.yield({ wait = "x" }) end,
              priority = -1,
            })
            world.register_field_action("tools:hidden", { menu = false, run = function() end })
            ok_dup, dup_err = perr(world.register_field_action, "tools:cut", { run = function() end })
            ok_run, run_err = perr(world.register_field_action, "tools:x", {})
            "#,
        );
    let (mut h, report) = load(&files, &["tools"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let owner = ScriptOwner::plugin("tools");
    assert!(
        h.global(&owner, "dup_err")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("already registered by plugin `tools`")
    );
    assert!(
        h.global(&owner, "run_err")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("`run`")
    );
    let ids = |list: Vec<pkrs_script::FieldActionInfo>| -> Vec<String> {
        list.into_iter().map(|i| i.id).collect()
    };
    // Chain order: priority -1 first.
    assert_eq!(
        ids(h.field_actions()),
        ["tools:flash", "tools:cut", "tools:hidden"]
    );
    // Party menu, not facing a tree, flash unavailable.
    assert!(ids(h.available_field_actions(&ctx(FieldActionSource::PartyMenu, None))).is_empty());
    run(&mut h, &owner, "flash_ok = true").unwrap();
    assert_eq!(
        ids(h.available_field_actions(&ctx(FieldActionSource::PartyMenu, Some("small_tree")))),
        ["tools:flash", "tools:cut"]
    );
    // Facing a tile: only terrain-bound actions.
    let tile = ctx(FieldActionSource::FacingTile, Some("small_tree"));
    assert_eq!(ids(h.available_field_actions(&tile)), ["tools:cut"]);
    let (info, _, status) = h.start_field_action("tools:cut", &tile).unwrap();
    assert_eq!(info.label, "text:tools.cut");
    assert_eq!(info.owner, owner);
    assert!(matches!(status, TaskStatus::Finished(_)));
    assert_eq!(h.global(&owner, "used").unwrap(), "tile@1,2".into());
    // A run that yields is a task the engine resumes.
    let (_, task, status) = h
        .start_field_action("tools:flash", &ctx(FieldActionSource::PartyMenu, None))
        .unwrap();
    assert!(matches!(status, TaskStatus::Waiting(_)));
    h.resume_task(task, "done".into()).unwrap();
    assert_eq!(h.global(&owner, "ui_wait").unwrap(), "done".into());
    assert!(h.start_field_action("nope", &tile).is_err());
}

#[test]
fn input_actions_are_validated_and_handed_to_the_engine() {
    let files = MemoryScripts::new()
        .with("plugins/res/plugin.toml", manifest("res", ""))
        .with(
            "plugins/res/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            input.register_action("res:toggle", { "r", "left_shift" })
            input.register_action("res:quiet")
            for i, args in ipairs({
              { "toggle" }, { "confirm" }, { "other:x" }, { "res:toggle" }, { "res:k", { 1 } },
            }) do
              local ok, err = perr(input.register_action, args[1], args[2])
              errors = (errors or "") .. i .. ":" .. tostring(err) .. "\n"
            end
            "#,
        );
    let (mut h, report) = load(&files, &["res"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let taken = h.take_input_actions();
    let names: Vec<(&str, Vec<String>)> = taken
        .iter()
        .map(|a| (a.name.as_str(), a.keys.clone()))
        .collect();
    assert_eq!(
        names,
        [
            ("res:toggle", vec!["r".to_owned(), "left_shift".to_owned()]),
            ("res:quiet", vec![]),
        ]
    );
    assert!(h.take_input_actions().is_empty(), "drained");
    assert_eq!(h.input_actions().len(), 2);
    let errors = h.global(&ScriptOwner::plugin("res"), "errors").unwrap();
    let errors = errors.as_str().unwrap();
    for needle in [
        "must be namespaced",
        "plugin `res` cannot register in the `other:` namespace",
        "already registered",
        "keys[1] must be a string",
    ] {
        assert!(errors.contains(needle), "{needle}: {errors}");
    }
    // The game may use plain names, but not built-ins.
    run(
        &mut h,
        &ScriptOwner::Game,
        "input.register_action('quick_save', { 'f5' })",
    )
    .unwrap();
    assert!(
        run(&mut h, &ScriptOwner::Game, "input.register_action('menu')")
            .unwrap_err()
            .contains("built-in action")
    );
    assert_eq!(h.take_input_actions()[0].name, "quick_save");
}

#[test]
fn battle_actions_add_menu_entries_and_run() {
    let files = MemoryScripts::new()
        .with("plugins/res/plugin.toml", manifest("res", ""))
        .with(
            "plugins/res/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            battle.register_action("res:focus", {
              label = "text:res.focus",
              available = function(ctx, side) return side.is_player end,
              run = function(ctx, payload) ctx.message("text:res.focused") end,
            })
            ok_dup, dup_err = perr(battle.register_action, "res:focus", { run = function() end })
            ok_builtin, builtin_err = perr(battle.register_action, "run", { run = function() end })
            "#,
        );
    let (h, report) = load(&files, &["res"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let owner = ScriptOwner::plugin("res");
    assert_eq!(h.battle_actions(), ["res:focus"]);
    let resources = battle_testkit::default_resources();
    let mut hooks = h.battle_hooks().with_moves(&resources.pack);
    let mut battle = Battle::new(
        battle_testkit::wild_1v1(),
        BattleConfig {
            trace_hooks: false,
            ..BattleConfig::default()
        },
        battle_testkit::default_resources(),
        RngPolicy::Fixed { seed: 11 },
        &mut hooks,
    )
    .unwrap();
    battle.step(BattleInput::default(), &mut hooks).unwrap();
    let player = SlotRef::new(SideId::Player, 0);
    let menu = battle.action_menu(player, &mut hooks).unwrap();
    let entry = menu.options.iter().find(|o| o.id == "res:focus").unwrap();
    assert_eq!(entry.label, "text:res.focus");
    let foe = battle
        .action_menu(SlotRef::new(SideId::Opponent, 0), &mut hooks)
        .unwrap();
    assert!(!foe.contains("res:focus"));
    let input = BattleInput::both(
        SideInput {
            slots: vec![BattleAction::Custom {
                id: "res:focus".into(),
                payload: vec![],
            }],
        },
        SideInput {
            slots: vec![BattleAction::Fight {
                move_slot: 0,
                target_side: SideId::Player,
                target_slot: 0,
            }],
        },
    );
    let events = battle.step(input, &mut hooks).unwrap();
    assert!(events.contains(&BattleEvent::Message {
        key: "text:res.focused".into()
    }));
    assert!(events.contains(&BattleEvent::CustomActionUsed {
        side: SideId::Player,
        slot: 0,
        id: "res:focus".into(),
        handled: true,
    }));
    let mut h = h;
    assert!(
        h.global(&owner, "dup_err")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("already registered")
    );
    assert!(
        h.global(&owner, "builtin_err")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("built-in menu entry")
    );
}

#[test]
fn plugin_save_namespace_snapshots_and_restores() {
    let files = MemoryScripts::new()
        .with("plugins/res/plugin.toml", manifest("res", ""))
        .with(
            "plugins/res/init.lua",
            r#"
            local function perr(...) local ok, e = pcall(...); return ok, tostring(e) end
            local P = plugin.define()
            plugin.save.uses = 2
            P.save.history = { "a", "b" }
            plugin.save.nested = { ok = true, bad = function() end }
            plugin.save.callback = function() end
            same = (P.save == plugin.save)
            "#,
        );
    let (mut h, report) = load(&files, &["res"]);
    assert!(report.failed.is_empty(), "{:?}", report.failed);
    let owner = ScriptOwner::plugin("res");
    assert_eq!(h.global(&owner, "same").unwrap(), true.into());
    let ScriptValue::Map(save) = h.plugin_save("res").unwrap() else {
        panic!("save must be a table")
    };
    assert_eq!(save["uses"], ScriptValue::Int(2));
    assert_eq!(
        save["history"],
        ScriptValue::List(vec!["a".into(), "b".into()])
    );
    let ScriptValue::Map(nested) = &save["nested"] else {
        panic!("nested")
    };
    assert_eq!(nested.len(), 1, "function value skipped");
    assert!(!save.contains_key("callback"), "function key skipped");
    assert!(h.plugin_save("ghost").is_none());
    assert_eq!(h.plugin_saves().len(), 1);

    let mut restored = BTreeMap::new();
    restored.insert("uses".to_owned(), ScriptValue::Int(9));
    h.restore_plugin_save("res", &ScriptValue::Map(restored))
        .unwrap();
    assert_eq!(
        run(
            &mut h,
            &owner,
            "return plugin.define().save.uses + (plugin.save.history and 1 or 0)"
        )
        .unwrap(),
        ScriptValue::Int(9)
    );
    assert!(h.restore_plugin_save("ghost", &ScriptValue::Nil).is_err());
}
