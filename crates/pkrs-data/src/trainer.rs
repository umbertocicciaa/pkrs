//! Trainer and AI profile schemas (§7.5).
//!
//! ```ron
//! // data/trainers/champion_aria.ron
//! Trainer(
//!   id: "champion_aria",
//!   party: [ (species: "emberkit", level: 30, moves: ["ember", "scratch"]) ],
//!   ai_profile: "profiles/champion_hard",
//! )
//! // data/trainers/profiles/champion_hard.ron
//! AiProfile(
//!   id: "champion_hard",
//!   tier: Smart,
//!   script: "scripts/ai/champion.lua",
//!   weights: (aggression: 0.8, switch: 0.6, predict: 0.9),
//!   allow_items: true,
//! )
//! ```

use serde::Deserialize;

use crate::error::{DataError, Result};

/// Built-in AI tier (implemented in Rust for speed and determinism).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Deserialize)]
pub enum AiTier {
    /// Random usable move and target.
    Random,
    /// Type-aware: best power × STAB × effectiveness.
    #[default]
    Basic,
    /// Type and stat matchups, KO awareness and switching heuristics.
    Smart,
}

/// Behaviour weights, each in `0.0..=1.0`.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(default)]
pub struct AiWeights {
    /// Preference for damage over safety.
    pub aggression: f32,
    /// Willingness to switch out of a bad matchup.
    pub switch: f32,
    /// How much the opponent's best reply is taken into account.
    pub predict: f32,
}

impl Default for AiWeights {
    fn default() -> Self {
        Self {
            aggression: 0.5,
            switch: 0.5,
            predict: 0.5,
        }
    }
}

impl AiWeights {
    /// Weights as integer permille (`0..=1000`) so AI arithmetic stays exact across platforms.
    #[must_use]
    pub fn permille(&self) -> (u16, u16, u16) {
        let p = |w: f32| (w.clamp(0.0, 1.0) * 1000.0).round() as u16;
        (p(self.aggression), p(self.switch), p(self.predict))
    }
}

fn default_true() -> bool {
    true
}

/// A trainer AI profile.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct AiProfile {
    /// Profile id (`champion_hard`).
    pub id: String,
    /// Built-in tier.
    #[serde(default)]
    pub tier: AiTier,
    /// Optional Lua override (`choose_action(view)`, §7.5; executed by `pkrs-script`).
    #[serde(default)]
    pub script: Option<String>,
    /// Behaviour weights.
    #[serde(default)]
    pub weights: AiWeights,
    /// May the trainer use items from its `items` list.
    #[serde(default = "default_true")]
    pub allow_items: bool,
    /// Use a healing item when the active creature's HP falls to this percent or below.
    #[serde(default = "default_heal_below")]
    pub heal_below_percent: u8,
}

fn default_heal_below() -> u8 {
    25
}

impl AiProfile {
    /// A profile with default weights for `tier`.
    #[must_use]
    pub fn builtin(id: impl Into<String>, tier: AiTier) -> Self {
        Self {
            id: id.into(),
            tier,
            script: None,
            weights: AiWeights::default(),
            allow_items: false,
            heal_below_percent: default_heal_below(),
        }
    }

    /// Parses an `AiProfile(...)` RON document and validates it.
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        let profile: Self = parse(source, file)?;
        profile.validate(file)?;
        Ok(profile)
    }

    /// Schema checks: weights in `0..=1`, `heal_below_percent ≤ 100`, `.lua` script path.
    pub fn validate(&self, file: &str) -> Result<()> {
        let bad = |message: String| {
            Err(DataError::Parse {
                file: file.into(),
                message,
            })
        };
        let w = self.weights;
        for (name, value) in [
            ("aggression", w.aggression),
            ("switch", w.switch),
            ("predict", w.predict),
        ] {
            if !(0.0..=1.0).contains(&value) {
                return bad(format!("weights.{name} must be in 0.0..=1.0 (got {value})"));
            }
        }
        if self.heal_below_percent > 100 {
            return bad("heal_below_percent must be ≤ 100".into());
        }
        if let Some(script) = &self.script {
            if !script.ends_with(".lua") || script.contains("..") {
                return bad(format!(
                    "script `{script}` must be a project-relative .lua path"
                ));
            }
        }
        Ok(())
    }
}

/// One party member of a trainer.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct TrainerMember {
    /// Species id.
    pub species: String,
    /// Level 1–100.
    pub level: u8,
    /// Explicit moves (default: from the learnset).
    #[serde(default)]
    pub moves: Vec<String>,
}

/// A trainer (`data/trainers/*.ron`), written `Trainer(...)` in RON.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename = "Trainer")]
pub struct TrainerDef {
    /// Trainer id.
    pub id: String,
    /// Display name text key.
    #[serde(default)]
    pub name: Option<String>,
    /// Party (1–6 members).
    pub party: Vec<TrainerMember>,
    /// AI profile id or `profiles/<id>` path.
    pub ai_profile: String,
    /// Items the AI may use when its profile allows it.
    #[serde(default)]
    pub items: Vec<String>,
}

impl TrainerDef {
    /// Parses a `Trainer(...)` RON document and validates it.
    pub fn parse_ron(source: &str, file: &str) -> Result<Self> {
        let trainer: Self = parse(source, file)?;
        trainer.validate(file)?;
        Ok(trainer)
    }

    /// Schema checks: 1–6 members, levels 1–100, at most 4 moves each.
    pub fn validate(&self, file: &str) -> Result<()> {
        let bad = |message: String| {
            Err(DataError::Parse {
                file: file.into(),
                message,
            })
        };
        if !(1..=6).contains(&self.party.len()) {
            return bad(format!(
                "party must have 1–6 members (got {})",
                self.party.len()
            ));
        }
        for (i, m) in self.party.iter().enumerate() {
            if !(1..=100).contains(&m.level) {
                return bad(format!("party[{i}].level must be 1–100 (got {})", m.level));
            }
            if m.moves.len() > 4 {
                return bad(format!("party[{i}] has more than 4 moves"));
            }
        }
        if self.ai_profile.is_empty() {
            return bad("ai_profile must not be empty".into());
        }
        Ok(())
    }

    /// The referenced profile id (`profiles/champion_hard` → `champion_hard`).
    #[must_use]
    pub fn ai_profile_id(&self) -> &str {
        self.ai_profile
            .strip_prefix("profiles/")
            .unwrap_or(&self.ai_profile)
    }
}

/// Built-in profile ids that need no data file.
pub const BUILTIN_PROFILES: [(&str, AiTier); 3] = [
    ("random", AiTier::Random),
    ("basic", AiTier::Basic),
    ("smart", AiTier::Smart),
];

/// Finds the profile `trainer` refers to among `profiles`, falling back to the built-in
/// `random` / `basic` / `smart` ids.
pub fn resolve_ai_profile(trainer: &TrainerDef, profiles: &[AiProfile]) -> Result<AiProfile> {
    let id = trainer.ai_profile_id();
    if let Some(p) = profiles.iter().find(|p| p.id == id) {
        return Ok(p.clone());
    }
    BUILTIN_PROFILES
        .iter()
        .find(|(name, _)| *name == id)
        .map(|(name, tier)| AiProfile::builtin(*name, *tier))
        .ok_or_else(|| DataError::UnknownReference {
            kind: "ai profile".into(),
            id: id.to_owned(),
        })
}

/// Cross-reference check (§8.2 stage 3): every trainer's `ai_profile` exists. Returns every
/// failure, not just the first.
#[must_use]
pub fn check_ai_profiles(trainers: &[TrainerDef], profiles: &[AiProfile]) -> Vec<DataError> {
    trainers
        .iter()
        .filter_map(|t| resolve_ai_profile(t, profiles).err())
        .collect()
}

fn parse<T: for<'de> Deserialize<'de>>(source: &str, file: &str) -> Result<T> {
    ron::Options::default()
        .with_default_extension(ron::extensions::Extensions::IMPLICIT_SOME)
        .from_str(source)
        .map_err(|e| DataError::Parse {
            file: file.into(),
            message: e.to_string(),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROFILE: &str = r#"
AiProfile(
  id: "champion_hard",
  tier: Smart,
  script: "scripts/ai/champion.lua",
  weights: (aggression: 0.8, switch: 0.6, predict: 0.9),
  allow_items: true,
)"#;

    const TRAINER: &str = r#"
Trainer(
  id: "champion_aria",
  party: [ (species: "emberkit", level: 30, moves: ["ember", "scratch"]) ],
  ai_profile: "profiles/champion_hard",
)"#;

    #[test]
    fn design_doc_examples_parse() {
        let p = AiProfile::parse_ron(PROFILE, "profiles/champion_hard.ron").unwrap();
        assert_eq!(p.tier, AiTier::Smart);
        assert_eq!(p.script.as_deref(), Some("scripts/ai/champion.lua"));
        assert_eq!(p.weights.permille(), (800, 600, 900));
        assert!(p.allow_items);
        assert_eq!(p.heal_below_percent, 25);

        let t = TrainerDef::parse_ron(TRAINER, "champion_aria.ron").unwrap();
        assert_eq!(t.ai_profile_id(), "champion_hard");
        assert_eq!(t.party[0].moves, ["ember", "scratch"]);
        assert_eq!(resolve_ai_profile(&t, &[p]).unwrap().id, "champion_hard");
    }

    #[test]
    fn defaults_and_builtin_profiles() {
        let p = AiProfile::parse_ron(r#"AiProfile(id: "x")"#, "x.ron").unwrap();
        assert_eq!(p.tier, AiTier::Basic);
        assert_eq!(p.weights, AiWeights::default());
        let t = TrainerDef::parse_ron(
            r#"Trainer(id: "youngster", party: [(species: "a", level: 3)], ai_profile: "random")"#,
            "y.ron",
        )
        .unwrap();
        assert_eq!(resolve_ai_profile(&t, &[]).unwrap().tier, AiTier::Random);
    }

    #[test]
    fn schema_errors() {
        assert!(
            AiProfile::parse_ron(r#"AiProfile(id: "x", weights: (aggression: 1.5))"#, "x.ron")
                .is_err()
        );
        assert!(
            AiProfile::parse_ron(r#"AiProfile(id: "x", script: "../evil.lua")"#, "x.ron").is_err()
        );
        assert!(AiProfile::parse_ron(r#"AiProfile(id: "x", tier: Genius)"#, "x.ron").is_err());
        assert!(
            TrainerDef::parse_ron(
                r#"Trainer(id: "t", party: [], ai_profile: "basic")"#,
                "t.ron"
            )
            .is_err()
        );
        assert!(
            TrainerDef::parse_ron(
                r#"Trainer(id: "t", party: [(species: "a", level: 0)], ai_profile: "basic")"#,
                "t.ron"
            )
            .is_err()
        );
    }

    #[test]
    fn unknown_profiles_are_all_reported() {
        let mk = |id: &str, profile: &str| TrainerDef {
            id: id.into(),
            name: None,
            party: vec![TrainerMember {
                species: "a".into(),
                level: 5,
                moves: vec![],
            }],
            ai_profile: profile.into(),
            items: vec![],
        };
        let errs = check_ai_profiles(
            &[
                mk("a", "smart"),
                mk("b", "profiles/missing"),
                mk("c", "nope"),
            ],
            &[],
        );
        let msgs: Vec<_> = errs.iter().map(ToString::to_string).collect();
        assert_eq!(
            msgs,
            ["unknown ai profile `missing`", "unknown ai profile `nope`"]
        );
    }
}
