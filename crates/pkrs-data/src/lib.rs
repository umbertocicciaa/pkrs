//! Data-driven content: RON schemas, loading, validation and cross-reference checks.
//!
//! - Schemas (§8.1): [`SpeciesDef`], [`MoveDef`], [`ItemDef`], [`AbilityDef`],
//!   [`TypeChart`], [`TrainerDef`] / [`AiProfile`], [`EncounterTable`], text ([`TextDb`]),
//!   plus statuses, terrains and effect primitives.
//! - [`RonCodec`]: the only place the `ron` crate is used (§4.5).
//! - [`GameData`]: the immutable content database of a project, loaded through the
//!   [`DataFiles`] port with folder merging, duplicate detection and ids interned into
//!   [`Handle`]s; `features.battles = false` skips battle data ([`LoadOptions`]).
//! - [`validate`]: §8.2 validation as vendor-free diagnostics: load errors with file and
//!   line/column, stage-2 schema rules and stage-3 cross-references (ids, sprites, `text:`
//!   keys).
//! - [`content`]: the mutable content database plugins register into (§9.4), merged into
//!   [`GameData`] by [`GameDataBuilder::merge_content`].

mod battle_pack;
mod battle_rules;
mod codec;
pub mod content;
mod database;
mod encounter;
mod error;
mod files;
mod growth;
mod intern;
mod move_def;
mod species;
mod text;
mod trainer;
mod type_chart;
pub mod validate;

pub use battle_pack::BattlePack;
pub use battle_rules::{
    BattleRules, ResidualKind, StatusDef, StatusKind, WeatherDef, residual_amount,
    status_blocked_by_types, weather_chip_blocked,
};
pub use codec::{Decoded, RonCodec};
pub use content::{
    AbilityDef, ContentDb, ContentDef, ContentEntry, ContentKind, ContentOverride, ContentSource,
    ContentValue, ContentValueError, EffectDef, ItemDef, Record, TerrainDef, is_content_id,
    parse_content,
};
pub use database::{DATA_ROOT, DataErrors, DataKind, GameData, GameDataBuilder, LoadOptions};
pub use encounter::{
    EncounterPick, EncounterSlot, EncounterTable, EncounterTables, MAX_ENCOUNTER_LEVEL,
    roll_encounter_rate,
};
pub use error::{DataError, Result};
pub use files::{DataFiles, MemoryDataFiles};
pub use growth::{GrowthRate, MAX_LEVEL};
pub use intern::{Handle, Keyed, Table};
pub use move_def::{EffectTarget, MoveCategory, MoveDef, MoveEffect, MoveTargetScope, StatId};
pub use species::{
    BaseStats, DEFAULT_CATCH_RATE, EvolutionDef, EvolutionTrigger, LearnsetEntry, SpeciesDef,
    SpeciesSprites, check_species_refs,
};
pub use text::{TEXT_PREFIX, TextDb, is_text_key};
pub use trainer::{
    AiProfile, AiTier, AiVisibility, AiWeights, BUILTIN_PROFILES, TrainerDef, TrainerMember,
    check_ai_profiles, resolve_ai_profile,
};
pub use type_chart::TypeChart;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: DataError = DataError::UnknownReference {
            kind: "move".into(),
            id: "ember".into(),
        };
        assert_eq!(err.to_string(), "unknown move `ember`");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
