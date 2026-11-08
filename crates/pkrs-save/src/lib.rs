//! Save system: save model, versioning, migrations and atomic writes (§12).
//!
//! - **Model.** [`SaveData`] `{ header, world, plugins }`: a [`SaveHeader`] (format version,
//!   game identity, slot-screen summary), the [`WorldState`] (flags, variables, bag, money,
//!   per-map [`MapState`] (§6.5), player position, party and RNG streams) and plugin save
//!   namespaces as engine-owned [`Value`]s.
//! - **Format.** Pretty-printed RON of the save's [`Value`] tree ([`RonCodec`], the only module
//!   that uses `ron`, §4.5). Typed structs convert to and from `Value` trees with
//!   [`to_value`] / [`from_value`].
//! - **Versioning.** [`Migrations`] runs engine steps `v1 → v2 → …` on the tree, then an
//!   optional [`GameMigration`] (`scripts/migrate.lua`), before typed decoding ([`SaveLoader`]).
//! - **Slots.** [`SaveStore`] writes slots atomically (temp file, `fsync`, rename, `.bak`)
//!   through the [`SaveFs`] port and falls back to the backup on load.

mod codec;
mod error;
mod fs;
mod migrate;
mod model;
mod serde_value;
mod store;
mod value;
mod world;

pub use codec::RonCodec;
pub use error::{Result, SaveError};
pub use fs::{SaveFs, StdFs};
pub use migrate::{GameMigration, MigrationFn, Migrations, SaveLoader, format_version};
pub use model::{
    CURRENT_FORMAT_VERSION, PlayerPlacement, RNG_BATTLE, RNG_ENCOUNTER, RNG_SCRIPT, SaveData,
    SaveHeader, SaveSummary, SavedCreature, sanitize_plugin_value,
};
pub use serde_value::{ValueError, from_value, to_value};
pub use store::{DEFAULT_SLOTS, Loaded, SAVE_EXTENSION, SaveSlot, SaveStore, SlotInfo};
pub use value::Value;
pub use world::{MapState, ObjectState, WorldState};

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use pkrs_core::{Direction, GridPos, RngState};

    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: SaveError = SaveError::UnsupportedVersion(7);
        assert_eq!(err.to_string(), "unsupported save version 7");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }

    fn full_save() -> SaveData {
        let mut world = WorldState::new();
        world.set_flag("met_rival", true);
        world.set_flag("lost", false);
        world.set_var("count", Some(Value::Int(3)));
        world.set_var("ratio", Some(Value::Float(0.5)));
        world.set_var(
            "list",
            Some(Value::List(vec![Value::from("a"), Value::map()])),
        );
        world.give_item("potion", 2);
        world.money = 1234;
        world.map_mut("harbor").update_object("guard", |o| {
            o.hidden = true;
            o.moved_to = Some((GridPos::new(-1, 4), Direction::Left));
        });
        world
            .map_mut("harbor")
            .update_object("7", |o| o.picked_up = true);
        world.map_mut("harbor").fire_once("intro");
        world.map_mut("cave").set_var("visits", Some(Value::Int(2)));
        let mut player = PlayerPlacement::new("harbor", GridPos::new(3, 5), Direction::Up);
        player.field_states.insert("surf".into(), true);
        world.player = Some(player);
        world.party = vec![SavedCreature {
            species: "sproutle".into(),
            level: 12,
            exp: 1500,
            moves: vec!["tackle".into(), "leaf_cut".into()],
        }];
        world.rng.insert(
            RNG_BATTLE.into(),
            RngState {
                seed: u64::MAX,
                steps: 17,
            },
        );
        world
            .rng
            .insert(RNG_ENCOUNTER.into(), RngState { seed: 1, steps: 0 });
        world.language = Some("it".into());
        let mut save = SaveData {
            header: SaveHeader {
                engine_version: "0.0.0".into(),
                game_id: "com.example.harbor".into(),
                game_version: "0.1.0".into(),
                timestamp: 1_700_000_000,
                play_time_ticks: 3600,
                summary: SaveSummary {
                    player_name: "Ash".into(),
                    location: "harbor".into(),
                    progress: 2,
                    party_size: 1,
                },
                ..SaveHeader::default()
            },
            world,
            plugins: BTreeMap::new(),
        };
        save.set_plugin(
            "resonance",
            Value::Map(BTreeMap::from([("charge".into(), Value::Int(3))])),
        );
        save
    }

    #[test]
    fn full_save_round_trips_through_ron() {
        let save = full_save();
        let text = save.to_ron().unwrap();
        let back = SaveLoader::new().decode(&text, "slot1.sav").unwrap();
        assert_eq!(back, save);
        assert!(text.contains("\"format_version\": 2"), "{text}");
        assert!(text.contains("\"facing\": \"up\""), "{text}");
    }

    #[test]
    fn non_finite_plugin_numbers_are_skipped() {
        let mut save = SaveData::default();
        save.set_plugin(
            "p",
            Value::Map(BTreeMap::from([
                ("nan".into(), Value::Float(f64::NAN)),
                ("ok".into(), Value::Float(1.5)),
                (
                    "nested".into(),
                    Value::List(vec![Value::Float(f64::INFINITY), Value::Int(1)]),
                ),
            ])),
        );
        let p = &save.plugins["p"];
        assert_eq!(p.get("nan"), None);
        assert_eq!(p.get("ok"), Some(&Value::Float(1.5)));
        assert_eq!(p.get("nested"), Some(&Value::List(vec![Value::Int(1)])));
        save.set_plugin("q", Value::Float(f64::NAN));
        assert!(!save.plugins.contains_key("q"));
        // The rest of the save still writes.
        let text = save.to_ron().unwrap();
        assert_eq!(SaveLoader::new().decode(&text, "s").unwrap(), save);
    }
}
