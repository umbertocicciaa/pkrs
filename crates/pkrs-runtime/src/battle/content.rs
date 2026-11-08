//! Battle content taken from the project's immutable content database ([`GameData`], §8.1)
//! and turned into a [`BattleSetup`] for `battle.start_wild` / `battle.start_trainer`
//! requests.
//!
//! The database holds (all optional except what a requested battle needs) the type chart
//! (`data/types.ron`), moves (`data/moves`, merged with every move plugins add, §9.4),
//! species, trainers and AI profiles; the trainer `player` (`data/trainers/player.ron`,
//! [`PLAYER_TRAINER`]) is **the player's party** until the save-backed party model lands
//! (§12). A project with `features.battles = false` has no battle content
//! ([`BattleContentError::BattlesDisabled`]).
//!
//! Stats follow the Gen-3 formula with zero IVs/EVs; a move without a `pp` field gets
//! [`DEFAULT_PP`] PP. A creature with no explicit moves (every wild creature) knows
//! the last four moves of its species' learnset up to its level
//! ([`SpeciesDef::moves_at_level`]); a species without a learnset (or with nothing learned
//! yet at that level) falls back to up to four damaging moves of its own types, then any
//! damaging moves, in id order.

use std::collections::BTreeMap;

use pkrs_assets::Vfs;
use pkrs_battle::{
    BattleConfig, BattleCreature, BattleResources, BattleSetup, BattleSideSetup, CreatureStats,
    MoveSlot, PartyIndex, RngPolicy, SideId, TrainerAi,
};
use pkrs_data::{
    AiProfile, AiTier, BattlePack, DataError, DataErrors, GameData, LoadOptions, MoveCategory,
    SpeciesDef, TrainerDef, resolve_ai_profile,
};
use pkrs_script::BattleRequest;

/// Trainer id whose party the player fields (`data/trainers/player.ron`).
pub const PLAYER_TRAINER: &str = "player";

/// PP of a move slot whose move data has no `pp` field.
pub const DEFAULT_PP: u8 = 20;

/// AI profile of wild creatures.
pub const WILD_AI_PROFILE: &str = "random";

/// Why a battle could not be set up.
#[derive(Debug, thiserror::Error)]
pub enum BattleContentError {
    /// The project's content failed to load (every problem).
    #[error(transparent)]
    Load(#[from] DataErrors),
    /// A battle references unknown content.
    #[error(transparent)]
    Data(#[from] DataError),
    /// `features.battles = false`: the project has no battle data.
    #[error("battles are disabled (`features.battles = false` in pkrs.toml)")]
    BattlesDisabled,
    /// No `data/trainers/player.ron`.
    #[error("no player party: add `data/trainers/{PLAYER_TRAINER}.ron`")]
    NoPlayerParty,
    /// A creature ended up with no usable move.
    #[error("species `{0}` has no moves (list them in the party or add damaging moves)")]
    NoMoves(String),
}

/// What kind of battle is running (decides texts and whether running is allowed).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleKind {
    /// A wild creature.
    Wild,
    /// A trainer battle against `name`.
    Trainer {
        /// Trainer display name.
        name: String,
    },
}

/// Everything a battle needs: setup, content and the opponent's AI.
#[derive(Clone, Debug)]
pub struct PreparedBattle {
    /// Wild or trainer.
    pub kind: BattleKind,
    /// Active slots per side (singles by default; doubles plugin uses `2`).
    pub config: BattleConfig,
    /// Both parties.
    pub setup: BattleSetup,
    /// Content the battle resolves moves against.
    pub resources: BattleResources,
    /// The opponent's AI.
    pub ai: TrainerAi,
    /// Texts and content names of the battle screens (§5.5); English engine texts unless the
    /// runtime sets the project's ([`with_text`](Self::with_text)).
    pub text: crate::locale::BattleText,
}

impl PreparedBattle {
    /// Uses `text` for the battle's texts and renames every creature to its localized species
    /// name (nicknames are the species name until nicknames land).
    #[must_use]
    pub fn with_text(mut self, text: crate::locale::BattleText) -> Self {
        for side in [&mut self.setup.player, &mut self.setup.opponent] {
            for creature in &mut side.party {
                creature.nickname = text.species(&creature.species_id);
            }
        }
        self.text = text;
        self
    }
}

/// Battle content of one project.
#[derive(Clone, Debug)]
pub struct BattleContent {
    pack: BattlePack,
    trainers: BTreeMap<String, TrainerDef>,
    profiles: Vec<AiProfile>,
}

impl BattleContent {
    /// Loads the content of the project mounted in `vfs` (no plugins) and takes its battle
    /// content ([`from_data`](Self::from_data)).
    pub fn load(vfs: &dyn Vfs) -> Result<Self, BattleContentError> {
        let data = crate::data::load_game_data(vfs, LoadOptions::default(), None)?;
        Self::from_data(&data)
    }

    /// The battle content of `data`: type chart, moves, species, trainers and AI profiles.
    /// Errors with [`BattleContentError::BattlesDisabled`] when `data` was loaded without
    /// battle data.
    pub fn from_data(data: &GameData) -> Result<Self, BattleContentError> {
        if !data.battles_enabled() {
            return Err(BattleContentError::BattlesDisabled);
        }
        Ok(Self {
            pack: data.battle_pack(),
            trainers: data
                .trainers()
                .records()
                .iter()
                .map(|t| (t.id.clone(), t.clone()))
                .collect(),
            profiles: data.ai_profiles().records().to_vec(),
        })
    }

    /// Content from parts (tests, tools).
    #[must_use]
    pub fn from_parts(
        pack: BattlePack,
        trainers: Vec<TrainerDef>,
        profiles: Vec<AiProfile>,
    ) -> Self {
        Self {
            pack,
            trainers: trainers.into_iter().map(|t| (t.id.clone(), t)).collect(),
            profiles,
        }
    }

    /// The moves, species and type chart.
    #[must_use]
    pub fn pack(&self) -> &BattlePack {
        &self.pack
    }

    /// Builds the battle `request` asks for; the AI rolls on `policy`'s opponent stream.
    pub fn prepare(
        &self,
        request: &BattleRequest,
        policy: &RngPolicy,
    ) -> Result<PreparedBattle, BattleContentError> {
        let player_def = self
            .trainers
            .get(PLAYER_TRAINER)
            .ok_or(BattleContentError::NoPlayerParty)?;
        let player_slots = player_def.battle_active_slots();
        let (kind, opponent, ai, side_size) = match request {
            BattleRequest::Wild { species, level } => {
                let creature = self.creature(species, *level, &[])?;
                let side = BattleSideSetup {
                    name: format!("Wild {}", creature.nickname),
                    party: vec![creature],
                    leading: vec![Some(0)],
                };
                let profile = AiProfile::builtin(WILD_AI_PROFILE, AiTier::Random);
                let ai = TrainerAi::for_battle(&profile, Vec::new(), policy, SideId::Opponent);
                (BattleKind::Wild, side, ai, 1u8)
            }
            BattleRequest::Trainer { id } => {
                let def = self
                    .trainers
                    .get(id)
                    .ok_or_else(|| DataError::UnknownReference {
                        kind: "trainer".into(),
                        id: id.clone(),
                    })?;
                let name = def.name.clone().unwrap_or_else(|| display_name(&def.id));
                let opp_slots = def.battle_active_slots();
                let side_size = player_slots.max(opp_slots);
                let side = self.side(def, &name, side_size)?;
                let profile = resolve_ai_profile(def, &self.profiles)?;
                let ai =
                    TrainerAi::for_battle(&profile, def.items.clone(), policy, SideId::Opponent);
                (BattleKind::Trainer { name }, side, ai, side_size)
            }
        };
        let player = self.side(player_def, "Player", side_size)?;
        let config = BattleConfig::with_side_size(side_size);
        Ok(PreparedBattle {
            kind,
            config,
            setup: BattleSetup { player, opponent },
            resources: BattleResources::new(self.pack.clone()),
            ai,
            text: crate::locale::BattleText::default(),
        })
    }

    fn side(
        &self,
        def: &TrainerDef,
        name: &str,
        side_size: u8,
    ) -> Result<BattleSideSetup, BattleContentError> {
        let party_len = def.party.len();
        let party = def
            .party
            .iter()
            .map(|m| self.creature(&m.species, m.level, &m.moves))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BattleSideSetup {
            name: name.to_owned(),
            party,
            leading: Self::leading_for_side(side_size, party_len),
        })
    }

    /// Leading party indices for `side_size` active slots.
    #[must_use]
    pub fn leading_for_side(side_size: u8, party_len: usize) -> Vec<Option<PartyIndex>> {
        (0..usize::from(side_size))
            .map(|slot| {
                u8::try_from(slot)
                    .ok()
                    .filter(|&i| usize::from(i) < party_len)
            })
            .collect()
    }

    /// A level-`level` creature of `species` with `moves` (or its default moves).
    pub fn creature(
        &self,
        species: &str,
        level: u8,
        moves: &[String],
    ) -> Result<BattleCreature, BattleContentError> {
        let def = self
            .pack
            .species
            .get(species)
            .ok_or_else(|| DataError::UnknownReference {
                kind: "species".into(),
                id: species.to_owned(),
            })?;
        let ids: Vec<String> = if moves.is_empty() {
            self.default_moves(def, level)
        } else {
            moves.to_vec()
        };
        let mut slots: [Option<MoveSlot>; 4] = Default::default();
        for (slot, id) in slots.iter_mut().zip(ids.iter()) {
            let mv = self
                .pack
                .move_by_id(id)
                .ok_or_else(|| DataError::UnknownReference {
                    kind: "move".into(),
                    id: id.clone(),
                })?;
            *slot = Some(MoveSlot {
                id: mv.id.clone(),
                priority: mv.priority,
                power: mv.power,
                pp: mv.pp.unwrap_or(DEFAULT_PP),
                max_pp: mv.pp.unwrap_or(DEFAULT_PP),
            });
        }
        if slots.iter().all(Option::is_none) {
            return Err(BattleContentError::NoMoves(species.to_owned()));
        }
        Ok(BattleCreature {
            species_id: def.id.clone(),
            types: def.types.clone(),
            nickname: display_name(&def.id),
            level,
            stats: level_stats(def, level),
            moves: slots,
        })
    }

    /// Species data errors that only show once moves are known: learnset moves and
    /// evolution targets that do not exist (§8.2 stage 3). Plugin moves count when the
    /// content was built from a [`GameData`] that merged the plugins' content database.
    #[must_use]
    pub fn check_species(&self) -> Vec<DataError> {
        pkrs_data::check_species_refs(&self.pack.species, &self.pack.moves)
    }

    /// Moves of a creature without explicit moves: its learnset up to `level`, else the
    /// damaging-move stand-in.
    fn default_moves(&self, species: &SpeciesDef, level: u8) -> Vec<String> {
        let learned = species.moves_at_level(level);
        if !learned.is_empty() {
            return learned;
        }
        let damaging = || {
            self.pack
                .moves
                .values()
                .filter(|m| m.category != MoveCategory::Status && m.power > 0)
        };
        let mut ids: Vec<String> = damaging()
            .filter(|m| species.types.contains(&m.type_id))
            .map(|m| m.id.clone())
            .collect();
        for m in damaging() {
            if ids.len() >= 4 {
                break;
            }
            if !ids.contains(&m.id) {
                ids.push(m.id.clone());
            }
        }
        ids.truncate(4);
        ids
    }
}

/// Gen-3 stats with zero IVs/EVs: `hp = 2·base·L/100 + L + 10`, others `2·base·L/100 + 5`.
#[must_use]
pub fn level_stats(species: &SpeciesDef, level: u8) -> CreatureStats {
    let l = u32::from(level);
    let stat = |base: u32| 2 * base * l / 100 + 5;
    CreatureStats::new(
        2 * species.base_stats.hp * l / 100 + l + 10,
        stat(species.base_stats.atk),
        stat(species.base_stats.def),
        stat(species.base_stats.spe),
    )
}

/// `"tall_grass"` → `"Tall grass"`: a readable name for a content id without a localized
/// name (no `name` field and no `<kind>.<id>` text key, §5.5).
#[must_use]
pub fn display_name(id: &str) -> String {
    let id = id.rsplit(':').next().unwrap_or(id);
    let mut out = String::with_capacity(id.len());
    for (i, ch) in id.chars().enumerate() {
        match ch {
            '_' | '-' | '.' => out.push(' '),
            c if i == 0 => out.extend(c.to_uppercase()),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_assets::MemoryVfs;
    use pkrs_data::{MoveDef, SpeciesDef, TrainerMember, TypeChart};

    fn vfs() -> MemoryVfs {
        let mut vfs = MemoryVfs::new();
        let files = [
            (
                "data/types.ron",
                r#"(types: ["normal", "grass"], chart: {})"#,
            ),
            (
                "data/moves/tackle.ron",
                r#"(id: "tackle", type: "normal", category: physical, power: 35, accuracy: 100, priority: 0, effects: [(effect: "damage")])"#,
            ),
            (
                "data/moves/leaf.ron",
                r#"(id: "leaf", type: "grass", category: special, power: 40, accuracy: 100, priority: 0, effects: [(effect: "damage")])"#,
            ),
            (
                "data/species/leafkit.ron",
                r#"(id: "leafkit", types: ["grass"], hp: 40, attack: 18, defense: 16, speed: 14)"#,
            ),
            (
                "data/trainers/player.ron",
                r#"Trainer(id: "player", party: [(species: "leafkit", level: 10, moves: ["tackle"])], ai_profile: "basic")"#,
            ),
            (
                "data/trainers/rival.ron",
                r#"Trainer(id: "rival", name: "Rival", party: [(species: "leafkit", level: 5)], ai_profile: "basic")"#,
            ),
        ];
        for (path, text) in files {
            vfs.insert(path, text.as_bytes().to_vec()).unwrap();
        }
        vfs
    }

    #[test]
    fn doubles_trainer_prepares_side_size_two() {
        let mut pack = BattlePack::new(TypeChart::from_parts(Vec::new(), Default::default()));
        pack.insert_species(
            SpeciesDef::parse_ron(
                r#"(id: "leafkit", types: ["grass"], hp: 40, attack: 18, defense: 16, speed: 14)"#,
                "leafkit.ron",
            )
            .unwrap(),
        )
        .unwrap();
        pack.insert_move(
            MoveDef::parse_ron(
                r#"(id: "tackle", type: "normal", category: physical, power: 35, accuracy: 100, priority: 0, effects: [(effect: "damage")])"#,
                "tackle.ron",
            )
            .unwrap(),
        )
        .unwrap();
        let mk_member = |species: &str, level: u8| TrainerMember {
            species: species.into(),
            level,
            moves: vec!["tackle".into()],
        };
        let trainers = vec![
            TrainerDef {
                id: PLAYER_TRAINER.into(),
                name: Some("Player".into()),
                party: vec![mk_member("leafkit", 10), mk_member("leafkit", 9)],
                ai_profile: "basic".into(),
                items: vec![],
                active_slots: Some(2),
            },
            TrainerDef {
                id: "twins".into(),
                name: Some("Twins".into()),
                party: vec![mk_member("leafkit", 6), mk_member("leafkit", 5)],
                ai_profile: "basic".into(),
                items: vec![],
                active_slots: Some(2),
            },
        ];
        let content = BattleContent::from_parts(pack, trainers, Vec::new());
        let prepared = content
            .prepare(
                &BattleRequest::Trainer { id: "twins".into() },
                &RngPolicy::Fixed { seed: 0 },
            )
            .unwrap();
        assert_eq!(prepared.config.side_size, 2);
        assert_eq!(prepared.setup.player.leading, vec![Some(0), Some(1)]);
        assert_eq!(prepared.setup.opponent.leading, vec![Some(0), Some(1)]);
    }

    #[test]
    fn loads_project_data_and_prepares_a_trainer_battle() {
        let content = BattleContent::load(&vfs()).unwrap();
        let prepared = content
            .prepare(
                &BattleRequest::Trainer { id: "rival".into() },
                &RngPolicy::Fixed { seed: 1 },
            )
            .unwrap();
        assert_eq!(
            prepared.kind,
            BattleKind::Trainer {
                name: "Rival".into()
            }
        );
        let hero = &prepared.setup.player.party[0];
        assert_eq!((hero.nickname.as_str(), hero.level), ("Leafkit", 10));
        assert_eq!(hero.stats, CreatureStats::new(28, 8, 8, 7));
        let foe = &prepared.setup.opponent.party[0];
        // No explicit moves: own-type damaging moves first, then the rest, in id order.
        let ids: Vec<&str> = foe.moves.iter().flatten().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["leaf", "tackle"]);
        assert_eq!(foe.moves[0].as_ref().unwrap().pp, DEFAULT_PP);
    }

    #[test]
    fn wild_battles_and_errors() {
        let content = BattleContent::load(&vfs()).unwrap();
        let policy = RngPolicy::Fixed { seed: 1 };
        let wild = content
            .prepare(
                &BattleRequest::Wild {
                    species: "leafkit".into(),
                    level: 3,
                },
                &policy,
            )
            .unwrap();
        assert_eq!(wild.kind, BattleKind::Wild);
        assert_eq!(wild.setup.opponent.name, "Wild Leafkit");
        let unknown = content.prepare(&BattleRequest::Trainer { id: "x".into() }, &policy);
        assert!(matches!(unknown, Err(BattleContentError::Data(_))));
        let empty = BattleContent::load(&MemoryVfs::new()).unwrap();
        let no_party = empty.prepare(&BattleRequest::Trainer { id: "x".into() }, &policy);
        assert!(matches!(no_party, Err(BattleContentError::NoPlayerParty)));
    }

    #[test]
    fn learnsets_replace_the_default_move_stand_in() {
        let mut vfs = vfs();
        vfs.insert(
            "data/species/budling.ron",
            br#"(id: "budling", types: ["grass"], hp: 30, attack: 10, defense: 10, speed: 10,
                learnset: [(level: 1, move: "tackle"), (level: 4, move: "leaf"), (level: 9, move: "nope")],
                evolutions: [(into: "missing", trigger: Level(9))])"#
                .to_vec(),
        )
        .unwrap();
        let content = BattleContent::load(&vfs).unwrap();
        let ids = |level| {
            let c = content.creature("budling", level, &[]).unwrap();
            c.moves
                .iter()
                .flatten()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(3), ["tackle"]);
        assert_eq!(ids(5), ["tackle", "leaf"]);
        // Learnset entries are checked once the moves are known.
        let errors: Vec<String> = content
            .check_species()
            .iter()
            .map(ToString::to_string)
            .collect();
        assert_eq!(errors, ["unknown move `nope`", "unknown species `missing`"]);
    }

    #[test]
    fn display_names_are_readable() {
        assert_eq!(display_name("tall_grass"), "Tall grass");
        assert_eq!(display_name("resonance:resonate"), "Resonate");
        assert_eq!(display_name(""), "");
    }
}
