//! Stage 2 (§8.2) rules checked on the loaded records, beyond the per-record checks that run
//! while decoding (learnset and evolution levels, accuracy and chance ranges, party sizes,
//! encounter slots, …).
//!
//! Each rule returns the text to point at with its diagnostic: a quoted value, or a field
//! name prefixed with [`FIELD`].

use pkrs_core::Diagnostic;

use crate::move_def::{MoveDef, MoveEffect};
use crate::species::SpeciesDef;

/// Prefix marking a field name (rather than a value) as the place to point at.
pub const FIELD: &str = "\u{0}field:";

/// Highest move priority (Gen-3 range is −7..=7).
pub const MAX_PRIORITY: i8 = 7;

fn field(name: &str) -> String {
    format!("{FIELD}{name}")
}

/// Species: one or two distinct types, non-zero base HP.
#[must_use]
pub fn species(sp: &SpeciesDef) -> Vec<(String, Diagnostic)> {
    let mut out = Vec::new();
    let owner = format!("species `{}`", sp.id);
    if sp.types.is_empty() {
        out.push((
            field("types"),
            Diagnostic::error("data::schema", format!("{owner} has no type"))
                .with_label("needs one or two types"),
        ));
    }
    if let Some(extra) = sp.types.get(2) {
        out.push((
            extra.clone(),
            Diagnostic::error(
                "data::schema",
                format!(
                    "{owner} has {} types; at most 2 are allowed",
                    sp.types.len()
                ),
            )
            .with_label("third type"),
        ));
    }
    for (i, t) in sp.types.iter().enumerate() {
        if sp.types[..i].contains(t) {
            out.push((
                t.clone(),
                Diagnostic::error("data::schema", format!("{owner} lists type `{t}` twice"))
                    .with_label("repeated type"),
            ));
        }
    }
    if sp.base_stats.hp == 0 {
        out.push((
            field("hp"),
            Diagnostic::error("data::schema", format!("{owner} has 0 base HP"))
                .with_label("must be at least 1"),
        ));
    }
    out
}

/// Moves: PP at least 1 when given, priority in −7..=7, stat-stage effects change 1–6 stages.
#[must_use]
pub fn move_def(mv: &MoveDef) -> Vec<(String, Diagnostic)> {
    let mut out = Vec::new();
    let owner = format!("move `{}`", mv.id);
    if mv.pp == Some(0) {
        out.push((
            field("pp"),
            Diagnostic::error("data::schema", format!("{owner} has 0 PP"))
                .with_label("must be at least 1"),
        ));
    }
    if !(-MAX_PRIORITY..=MAX_PRIORITY).contains(&mv.priority) {
        out.push((
            field("priority"),
            Diagnostic::error(
                "data::schema",
                format!("{owner} has priority {}; it must be between -{MAX_PRIORITY} and {MAX_PRIORITY}", mv.priority),
            )
            .with_label("out of range"),
        ));
    }
    for effect in &mv.effects {
        if let MoveEffect::StatStage { stages, .. } = effect
            && (*stages == 0 || stages.unsigned_abs() > 6)
        {
            out.push((
                field("stages"),
                Diagnostic::error(
                    "data::schema",
                    format!(
                        "{owner} changes a stat by {stages} stages; it must be 1 to 6 up or down"
                    ),
                )
                .with_label("out of range"),
            ));
        }
    }
    out
}
