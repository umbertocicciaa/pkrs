//! The player's party for the play session (§7.3, §8.1): species, level, experience and
//! moves of every member, kept between battles.
//!
//! Until the save-backed party lands (§12, M6), the party is built from
//! `data/trainers/player.ron` on the first battle ([`PlayerParty::from_side`]) and then lives
//! in memory: experience, level ups, learned moves and evolutions carry over to every later
//! battle of the session. Field HP is kept between battles for menus and saves; every battle still
//! starts at full HP and PP until saves own the party (§7.3).

use pkrs_battle::{BattleSideSetup, CreatureStats};
use pkrs_data::{BattlePack, MAX_LEVEL, SpeciesDef};

use crate::battle::content::{BattleContent, BattleContentError, display_name, level_stats};

/// Moves a creature can know at once.
pub const MAX_MOVES: usize = 4;

/// One member of the player's party.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartyMember {
    /// Species id.
    pub species: String,
    /// Level (`1..=100`).
    pub level: u8,
    /// Total experience.
    pub exp: u32,
    /// Known moves (at most [`MAX_MOVES`]), in slot order.
    pub moves: Vec<String>,
    /// Current HP in the field (`None` = full HP at the current level).
    pub hp: Option<u32>,
}

/// What [`PartyMember::learn`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Learn {
    /// The move went into a free slot.
    Learned,
    /// The member already knows it.
    AlreadyKnown,
    /// Four moves are known: the player chooses one to forget (or not to learn it).
    Full,
}

impl PartyMember {
    /// A level-`level` member of `species` with the minimum exp for that level.
    #[must_use]
    pub fn new(species: &SpeciesDef, level: u8, moves: Vec<String>) -> Self {
        let level = level.clamp(1, MAX_LEVEL);
        Self {
            species: species.id.clone(),
            level,
            exp: species.exp_for_level(level),
            moves,
            hp: None,
        }
    }

    /// Display name (the species name until nicknames land).
    #[must_use]
    pub fn name(&self) -> String {
        display_name(&self.species)
    }

    /// Stats at the current level (Gen-3 formula, zero IVs/EVs).
    #[must_use]
    pub fn stats(&self, species: &SpeciesDef) -> CreatureStats {
        level_stats(species, self.level)
    }

    /// Current HP in the field (full when [`Self::hp`] is unset).
    #[must_use]
    pub fn current_hp(&self, species: &SpeciesDef) -> u32 {
        let max = self.stats(species).max_hp;
        self.hp.unwrap_or(max).min(max)
    }

    /// Sets current HP, clamped to max HP at this level.
    pub fn set_hp(&mut self, hp: u32, species: &SpeciesDef) {
        let max = self.stats(species).max_hp;
        self.hp = Some(hp.min(max));
    }

    /// Heals by `amount`, capped at max HP.
    pub fn heal(&mut self, amount: u32, species: &SpeciesDef) {
        let cur = self.current_hp(species);
        self.set_hp(cur.saturating_add(amount), species);
    }

    /// Sets current HP to the maximum at this level.
    pub fn heal_full(&mut self, species: &SpeciesDef) {
        self.hp = None;
        let _ = species;
    }

    /// `true` when current HP is zero.
    #[must_use]
    pub fn fainted(&self, species: &SpeciesDef) -> bool {
        self.current_hp(species) == 0
    }

    /// Deals `amount` damage, floored at 0 HP.
    pub fn damage(&mut self, amount: u32, species: &SpeciesDef) {
        let cur = self.current_hp(species);
        self.set_hp(cur.saturating_sub(amount), species);
    }

    /// Adds `amount` exp (capped at the exp of level 100) and levels up along `species`'
    /// growth curve. Returns every level gained, in order (empty when none).
    pub fn gain_exp(&mut self, amount: u32, species: &SpeciesDef) -> Vec<u8> {
        let cap = species.exp_for_level(MAX_LEVEL);
        self.exp = self.exp.saturating_add(amount).min(cap);
        let new_level = species.growth.level_for_exp(self.exp).max(self.level);
        let gained = (self.level.saturating_add(1)..=new_level).collect();
        self.level = new_level;
        gained
    }

    /// Learns `move_id` into a free slot, if there is one.
    pub fn learn(&mut self, move_id: &str) -> Learn {
        if self.moves.iter().any(|m| m == move_id) {
            Learn::AlreadyKnown
        } else if self.moves.len() >= MAX_MOVES {
            Learn::Full
        } else {
            self.moves.push(move_id.to_owned());
            Learn::Learned
        }
    }

    /// Forgets the move in `slot` for `move_id`; returns the forgotten move (`None`: no such
    /// slot, nothing changed).
    pub fn replace_move(&mut self, slot: usize, move_id: &str) -> Option<String> {
        let old = self.moves.get_mut(slot)?;
        Some(std::mem::replace(old, move_id.to_owned()))
    }

    /// Becomes `into` (same level; exp is raised to the new curve's minimum for the level if
    /// the curves differ).
    pub fn evolve(&mut self, into: &SpeciesDef) {
        self.species = into.id.clone();
        self.exp = self.exp.max(into.exp_for_level(self.level));
    }
}

/// The player's party.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerParty {
    members: Vec<PartyMember>,
}

impl PlayerParty {
    /// A party of `members`.
    #[must_use]
    pub fn new(members: Vec<PartyMember>) -> Self {
        Self { members }
    }

    /// The party a battle side starts with (the first battle's `player.ron` side): species,
    /// level and moves of every creature, exp at the minimum of its level.
    #[must_use]
    pub fn from_side(side: &BattleSideSetup, pack: &BattlePack) -> Self {
        let members = side
            .party
            .iter()
            .map(|c| {
                let moves = c.moves.iter().flatten().map(|m| m.id.clone()).collect();
                match pack.species.get(&c.species_id) {
                    Some(species) => PartyMember::new(species, c.level, moves),
                    None => PartyMember {
                        species: c.species_id.clone(),
                        level: c.level,
                        exp: 0,
                        moves,
                        hp: Some(c.stats.max_hp),
                    },
                }
            })
            .collect();
        Self { members }
    }

    /// Members in party order (party index `i` in battle is member `i`).
    #[must_use]
    pub fn members(&self) -> &[PartyMember] {
        &self.members
    }

    /// Member `index`.
    #[must_use]
    pub fn member(&self, index: usize) -> Option<&PartyMember> {
        self.members.get(index)
    }

    /// Member `index`, mutably.
    pub fn member_mut(&mut self, index: usize) -> Option<&mut PartyMember> {
        self.members.get_mut(index)
    }

    /// Adds `member` at the end; `false` (and nothing added) when the party already holds
    /// [`MAX_PARTY`](pkrs_script::MAX_PARTY) creatures.
    pub fn push(&mut self, member: PartyMember) -> bool {
        if self.members.len() >= pkrs_script::MAX_PARTY {
            return false;
        }
        self.members.push(member);
        true
    }

    /// Swaps members at `a` and `b` when both indices are valid and distinct.
    pub fn swap_members(&mut self, a: usize, b: usize) -> bool {
        if a == b {
            return false;
        }
        let len = self.members.len();
        if a >= len || b >= len {
            return false;
        }
        self.members.swap(a, b);
        true
    }

    /// The battle side of this party (full HP and PP, current species, levels and moves),
    /// named `name`.
    pub fn battle_side(
        &self,
        content: &BattleContent,
        name: &str,
        side_size: u8,
    ) -> Result<BattleSideSetup, BattleContentError> {
        let party_len = self.members.len();
        let party = self
            .members
            .iter()
            .map(|m| content.creature(&m.species, m.level, &m.moves))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(BattleSideSetup {
            name: name.to_owned(),
            party,
            leading: crate::battle::content::BattleContent::leading_for_side(side_size, party_len),
        })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;
    use pkrs_data::{GrowthRate, TypeChart};

    fn species(growth: GrowthRate) -> SpeciesDef {
        SpeciesDef::parse_ron(
            &format!(
                r#"(id: "sproutling", types: ["grass"], hp: 40, attack: 30, defense: 30, speed: 25,
                    growth: {growth:?})"#
            ),
            "s.ron",
        )
        .unwrap()
    }

    #[test]
    fn exp_levels_up_along_the_growth_curve() {
        let s = species(GrowthRate::MediumFast);
        let mut m = PartyMember::new(&s, 5, vec!["tackle".into()]);
        assert_eq!(m.exp, 125);
        assert!(m.gain_exp(90, &s).is_empty(), "215 < 216 (level 6)");
        assert_eq!((m.level, m.exp), (5, 215));
        assert_eq!(m.gain_exp(1, &s), [6]);
        // Several levels at once, reported one by one.
        assert_eq!(m.gain_exp(1_000 - 216, &s), [7, 8, 9, 10]);
        assert_eq!((m.level, m.exp), (10, 1_000));
        // Capped at level 100.
        assert_eq!(m.gain_exp(u32::MAX, &s).last(), Some(&100));
        assert_eq!((m.level, m.exp), (100, 1_000_000));
        assert!(m.gain_exp(5, &s).is_empty());
        assert_eq!(m.exp, 1_000_000);
    }

    #[test]
    fn level_up_recalculates_stats() {
        let s = species(GrowthRate::Fast);
        let mut m = PartyMember::new(&s, 5, Vec::new());
        assert_eq!(m.stats(&s), CreatureStats::new(19, 8, 8, 7));
        m.gain_exp(GrowthRate::Fast.exp_for_level(20) - m.exp, &s);
        assert_eq!(m.level, 20);
        // hp = 2·40·20/100 + 20 + 10, others 2·base·20/100 + 5.
        assert_eq!(m.stats(&s), CreatureStats::new(46, 17, 17, 15));
    }

    #[test]
    fn learning_and_replacing_moves() {
        let s = species(GrowthRate::MediumFast);
        let mut m = PartyMember::new(&s, 5, vec!["a".into(), "b".into(), "c".into()]);
        assert_eq!(m.learn("b"), Learn::AlreadyKnown);
        assert_eq!(m.learn("d"), Learn::Learned);
        assert_eq!(m.learn("e"), Learn::Full);
        assert_eq!(m.moves, ["a", "b", "c", "d"]);
        assert_eq!(m.replace_move(1, "e").as_deref(), Some("b"));
        assert_eq!(m.moves, ["a", "e", "c", "d"]);
        assert_eq!(m.replace_move(4, "f"), None);
    }

    #[test]
    fn evolving_keeps_level_and_moves() {
        let s = species(GrowthRate::MediumFast);
        let mut into = species(GrowthRate::Slow);
        into.id = "bloomling".into();
        let mut m = PartyMember::new(&s, 6, vec!["a".into()]);
        m.evolve(&into);
        assert_eq!((m.species.as_str(), m.level), ("bloomling", 6));
        assert_eq!(m.exp, GrowthRate::Slow.exp_for_level(6));
        assert_eq!(m.moves, ["a"]);
        assert_eq!(m.name(), "Bloomling");
    }

    #[test]
    fn party_from_a_battle_side() {
        let mut pack = BattlePack::new(TypeChart::from_parts(Vec::new(), Default::default()));
        pack.insert_species(species(GrowthRate::MediumFast))
            .unwrap();
        let mut setup = pkrs_battle::battle_testkit::wild_1v1();
        setup.player.party[0].species_id = "sproutling".into();
        let party = PlayerParty::from_side(&setup.player, &pack);
        let m = party.member(0).unwrap();
        assert_eq!((m.level, m.exp), (5, 125));
        assert_eq!(m.moves, ["tackle"]);
        // Unknown species keep their level with no exp.
        let unknown = PlayerParty::from_side(&setup.opponent, &pack);
        assert_eq!(unknown.members()[0].exp, 0);
    }
}
