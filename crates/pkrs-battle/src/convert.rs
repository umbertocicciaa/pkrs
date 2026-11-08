//! Conversion from content loaders into battle-owned types (§7.2 decoupling).

use crate::error::Result;
use crate::model::BattleSetup;

/// Content-side party snapshot (implemented by `pkrs-data` in a later issue).
///
/// Keeping this trait in `pkrs-battle` ensures the core never depends on RON schema types.
pub trait IntoBattleSetup {
    /// Builds a validated [`BattleSetup`].
    fn into_battle_setup(self) -> Result<BattleSetup>;
}

/// Helper for tests and fixtures until `pkrs-data` provides real conversions.
#[allow(clippy::expect_used)]
pub mod testkit {
    use std::collections::HashMap;

    use pkrs_data::{BattlePack, MoveCategory, MoveDef, MoveEffect, TypeChart};

    use crate::model::{
        BattleConfig, BattleCreature, BattleSetup, BattleSideSetup, CreatureStats, MoveSlot,
    };
    use crate::resources::BattleResources;

    fn default_type_chart() -> TypeChart {
        let mut chart = HashMap::new();
        chart.insert(("fire".into(), "water".into()), 0.5);
        chart.insert(("water".into(), "fire".into()), 2.0);
        TypeChart::from_parts(vec!["normal".into(), "fire".into(), "water".into()], chart)
    }

    fn tackle_move() -> MoveDef {
        MoveDef {
            id: "tackle".into(),
            type_id: "normal".into(),
            category: MoveCategory::Physical,
            power: 35,
            accuracy: 100,
            priority: 0,
            targeting: Default::default(),
            effects: vec![MoveEffect::Damage],
        }
    }

    /// Default battle content for unit/integration tests.
    pub fn default_resources() -> BattleResources {
        let mut pack = BattlePack::new(default_type_chart());
        pack.insert_move(tackle_move()).expect("tackle");
        BattleResources::new(pack)
    }

    /// Minimal 1v1 wild battle for tests.
    pub fn wild_1v1() -> BattleSetup {
        let mk = |name: &str, hp: u32, atk: u32, def: u32, spe: u32, power: u16| BattleCreature {
            species_id: name.into(),
            types: vec!["normal".into()],
            nickname: name.into(),
            level: 5,
            stats: CreatureStats::new(hp, atk, def, spe),
            moves: [
                Some(MoveSlot {
                    id: "tackle".into(),
                    priority: 0,
                    power,
                    pp: 35,
                    max_pp: 35,
                }),
                None,
                None,
                None,
            ],
        };
        BattleSetup {
            player: BattleSideSetup {
                name: "Player".into(),
                party: vec![mk("hero", 40, 20, 10, 15, 35)],
                leading: vec![Some(0)],
            },
            opponent: BattleSideSetup {
                name: "Wild".into(),
                party: vec![mk("foe", 30, 15, 8, 10, 30)],
                leading: vec![Some(0)],
            },
        }
    }

    /// Default singles config.
    pub fn singles_config() -> BattleConfig {
        BattleConfig::with_side_size(1)
    }
}
