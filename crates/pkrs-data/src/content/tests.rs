use super::*;
use crate::move_def::MoveCategory;

const EMBER: &str = r#"(
    id: "ember", type: "fire", category: special, power: 40, accuracy: 100, priority: 0,
    effects: [(effect: "damage"), (effect: "apply_status", status: "burn", chance: 10)],
)"#;

fn plugin(id: &str) -> ContentSource {
    ContentSource::Plugin(id.into())
}

fn one(kind: ContentKind, src: &str) -> Record {
    let mut records = parse_content(kind, src, "test.ron").unwrap();
    assert_eq!(records.len(), 1);
    records.remove(0)
}

fn map(entries: &[(&str, ContentValue)]) -> ContentValue {
    ContentValue::Map(
        entries
            .iter()
            .map(|(k, v)| ((*k).to_owned(), v.clone()))
            .collect(),
    )
}

fn s(v: &str) -> ContentValue {
    ContentValue::Str(v.into())
}

#[test]
fn files_hold_one_record_or_a_list_with_override_flags() {
    let single = one(ContentKind::Move, EMBER);
    assert!(!single.override_existing);
    assert_eq!(single.def.id(), "ember");
    let list = format!(
        "[{EMBER}, (id: \"tackle\", type: \"normal\", category: physical, power: 40, \
         accuracy: 100, priority: 0, effects: [], override: true)]"
    );
    let records = parse_content(ContentKind::Move, &list, "moves.ron").unwrap();
    let flags: Vec<(&str, bool)> = records
        .iter()
        .map(|r| (r.def.id(), r.override_existing))
        .collect();
    assert_eq!(flags, [("ember", false), ("tackle", true)]);
    let terrain = one(
        ContentKind::Terrain,
        "(id: \"mud\", requires_field: Some(\"boots\"), override: true)",
    );
    assert!(terrain.override_existing);
    assert_eq!(
        terrain.def,
        ContentDef::Terrain(TerrainDef {
            id: "mud".into(),
            passable: true,
            requires_field: Some("boots".into()),
            encounter: None,
        })
    );
}

#[test]
fn parse_errors_name_the_file() {
    let err = parse_content(ContentKind::Item, "(name: \"x\")", "data/items/x.ron").unwrap_err();
    assert!(err.to_string().contains("data/items/x.ron"), "{err}");
}

#[test]
fn duplicates_are_errors_and_overrides_are_reported() {
    let mut db = ContentDb::new();
    assert_eq!(
        db.insert(one(ContentKind::Move, EMBER), ContentSource::Game)
            .unwrap(),
        None
    );
    let err = db
        .insert(one(ContentKind::Move, EMBER), plugin("fire-pack"))
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "duplicate move `ember`: already defined by the game; plugin `fire-pack` must declare \
         `override: true` to replace it"
    );
    let mut record = one(ContentKind::Move, EMBER);
    record.override_existing = true;
    if let ContentDef::Move(mv) = &mut record.def {
        mv.power = 60;
    }
    let ov = db.insert(record, plugin("fire-pack")).unwrap().unwrap();
    assert_eq!(
        ov.to_string(),
        "move `ember` from the game overridden by plugin `fire-pack`"
    );
    assert_eq!(db.move_def("ember").unwrap().power, 60);
    assert_eq!(
        db.get(ContentKind::Move, "ember").unwrap().source,
        plugin("fire-pack")
    );
    assert_eq!(db.overrides(), [ov]);
    // Same id, other kind: no clash.
    db.insert(
        one(ContentKind::Item, "(id: \"ember\")"),
        ContentSource::Game,
    )
    .unwrap();
    assert_eq!(db.ids(ContentKind::Item), ["ember"]);
    assert_eq!(db.item("ember").unwrap().pocket, "items");
}

#[test]
fn override_of_a_missing_id_defines_it() {
    let mut db = ContentDb::new();
    let record = one(ContentKind::Ability, "(id: \"kindle\", override: true)");
    assert_eq!(db.insert(record, plugin("a")).unwrap(), None);
    assert!(db.contains(ContentKind::Ability, "kindle"));
    assert!(db.overrides().is_empty());
}

#[test]
fn namespaces_belong_to_their_plugin() {
    let mut db = ContentDb::new();
    let own = one(ContentKind::Effect, "(id: \"resonance:echo\")");
    db.insert(own, plugin("resonance")).unwrap();
    let foreign = one(ContentKind::Effect, "(id: \"resonance:other\")");
    let err = db.insert(foreign.clone(), plugin("thief")).unwrap_err();
    assert!(err.to_string().contains("`resonance:` namespace"), "{err}");
    assert!(db.insert(foreign, ContentSource::Game).is_err());
    // Overriding another plugin's existing entry is allowed.
    let mut over = one(ContentKind::Effect, "(id: \"resonance:echo\")");
    over.override_existing = true;
    assert!(db.insert(over, plugin("thief")).unwrap().is_some());
    // Malformed ids.
    for bad in ["Ember", "a:b:c", ":x", "x:"] {
        let r = Record {
            def: ContentDef::Item(ItemDef {
                id: bad.into(),
                name: String::new(),
                description: String::new(),
                price: 0,
                pocket: "items".into(),
            }),
            override_existing: false,
        };
        assert!(db.insert(r, ContentSource::Game).is_err(), "{bad}");
    }
}

#[test]
fn schema_checks_run_on_insert() {
    let mut db = ContentDb::new();
    let bad_move = EMBER.replace("accuracy: 100", "accuracy: 101");
    let err = db
        .insert(one(ContentKind::Move, &bad_move), ContentSource::Game)
        .unwrap_err();
    assert!(err.to_string().contains("accuracy"), "{err}");
    let bad_chance = EMBER.replace("chance: 10", "chance: 200");
    assert!(
        db.insert(one(ContentKind::Move, &bad_chance), ContentSource::Game)
            .is_err()
    );
    let bad_status = "(id: \"doze\", kind: major, sleep_turns: Some((3, 1)))";
    assert!(
        db.insert(one(ContentKind::Status, bad_status), ContentSource::Game)
            .is_err()
    );
    let bad_terrain = "(id: \"mud\", requires_field: Some(\"\"))";
    assert!(
        db.insert(one(ContentKind::Terrain, bad_terrain), ContentSource::Game)
            .is_err()
    );
    assert!(db.is_empty());
}

#[test]
fn records_build_from_dynamic_values() {
    let value = map(&[
        ("id", s("mudworks:mud_bomb")),
        ("type", s("ground")),
        ("category", s("special")),
        ("power", ContentValue::Int(65)),
        ("accuracy", ContentValue::Int(85)),
        ("priority", ContentValue::Int(0)),
        (
            "effects",
            ContentValue::List(vec![
                map(&[("effect", s("damage"))]),
                map(&[
                    ("effect", s("stat_stage")),
                    ("stat", s("accuracy")),
                    ("stages", ContentValue::Int(-1)),
                    ("target", s("target")),
                    ("chance", ContentValue::Int(30)),
                ]),
            ]),
        ),
        ("override", ContentValue::Bool(true)),
    ]);
    let record = ContentDef::from_value(ContentKind::Move, value, "data.register_move").unwrap();
    assert!(record.override_existing);
    let ContentDef::Move(mv) = &record.def else {
        panic!("not a move")
    };
    assert_eq!(mv.category, MoveCategory::Special);
    assert_eq!(mv.effects.len(), 2);

    // Data enum variants as one-entry tables, tuples as lists, empty table as empty list.
    let status = map(&[
        ("id", s("soak")),
        ("kind", s("volatile")),
        (
            "residual",
            map(&[("max_hp_fraction", map(&[("denom", ContentValue::Int(16))]))]),
        ),
        (
            "sleep_turns",
            ContentValue::List(vec![ContentValue::Int(1), ContentValue::Int(3)]),
        ),
        ("immune_types", ContentValue::Map(BTreeMap::new())),
    ]);
    let record = ContentDef::from_value(ContentKind::Status, status, "s").unwrap();
    let ContentDef::Status(def) = record.def else {
        panic!("not a status")
    };
    assert_eq!(def.sleep_turns, Some((1, 3)));
    assert!(def.residual.is_some());

    let err = ContentDef::from_value(
        ContentKind::Move,
        map(&[("id", s("x"))]),
        "data.register_move",
    )
    .unwrap_err();
    assert!(err.to_string().contains("data.register_move"), "{err}");
    let err = ContentDef::from_value(
        ContentKind::Item,
        map(&[("id", s("x")), ("override", s("yes"))]),
        "data.register_item",
    )
    .unwrap_err();
    assert!(err.to_string().contains("override"), "{err}");
    assert!(ContentDef::from_value(ContentKind::Item, s("x"), "c").is_err());
}

#[test]
fn kinds_round_trip_by_name() {
    for kind in ContentKind::ALL {
        assert_eq!(ContentKind::parse(kind.name()), Some(*kind));
        assert!(!kind.folder().is_empty());
    }
    assert_eq!(ContentKind::parse("species"), None);
}
