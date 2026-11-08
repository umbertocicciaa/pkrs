//! Data-driven content: RON schemas, loading, validation and cross-reference checks.
//!
//! See `docs/design.md` §7.3 for battle-related schemas (full content pipeline in M6) and
//! [`content`] for the merged content database plugins register into (§8.1, §9.4).

mod battle_pack;
mod battle_rules;
pub mod content;
mod error;
mod growth;
mod move_def;
mod species;
mod trainer;
mod type_chart;

pub use battle_pack::BattlePack;
pub use battle_rules::{
    BattleRules, ResidualKind, StatusDef, StatusKind, WeatherDef, residual_amount,
    status_blocked_by_types, weather_chip_blocked,
};
pub use content::{
    AbilityDef, ContentDb, ContentDef, ContentEntry, ContentKind, ContentOverride, ContentSource,
    ContentValue, ContentValueError, EffectDef, ItemDef, Record, TerrainDef, is_content_id,
    parse_content,
};
pub use error::{DataError, Result};
pub use growth::{GrowthRate, MAX_LEVEL};
pub use move_def::{EffectTarget, MoveCategory, MoveDef, MoveEffect, StatId};
pub use species::{EvolutionDef, EvolutionTrigger, LearnsetEntry, SpeciesDef, check_species_refs};
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
