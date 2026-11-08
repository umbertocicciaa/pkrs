//! The merged content database (§8.1, §9.4 step 4): moves, abilities, items, statuses,
//! terrains and data effect primitives from the game's `data/` folder, from each plugin's
//! `data/` folder (in load order) and from Lua `data.register_*` calls.
//!
//! - **Ids** are `snake_case` (`ember`) or namespaced `plugin_id:snake_case`
//!   (`resonance:resonate`). A plugin may only create ids in its own namespace (or plain ids);
//!   the game may only create plain ids. Overriding an existing entry is always allowed.
//! - **Duplicates** are an error ([`DataError::DuplicateContent`]) unless the later record
//!   declares `override: true` (RON) / `override = true` (Lua). An override replaces the
//!   entry and is returned as a [`ContentOverride`] so the caller can log it.
//! - **Files** hold one record or a list of records ([`parse_content`]).

mod value;

use core::fmt;
use std::collections::BTreeMap;

use pkrs_core::{is_action_name, is_plugin_id};
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::codec::RonCodec;

pub use value::{ContentValue, ContentValueError};

use crate::battle_rules::StatusDef;
use crate::error::{DataError, Result};
use crate::move_def::{MoveDef, MoveEffect};

/// The kinds of content plugins (and the game) can register (§9.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentKind {
    /// Moves (`data/moves/*.ron`, `data.register_move`).
    Move,
    /// Abilities (`data/abilities/*.ron`, `data.register_ability`).
    Ability,
    /// Items (`data/items/*.ron`, `data.register_item`).
    Item,
    /// Statuses (`data/statuses/*.ron`, `data.register_status`).
    Status,
    /// Terrains (`data/terrains/*.ron`, `data.register_terrain`), §6.3.
    Terrain,
    /// Data effect primitives (`data/effects/*.ron`, `data.register_effect`).
    Effect,
}

impl ContentKind {
    /// Every kind, in merge order.
    pub const ALL: &'static [ContentKind] = &[
        Self::Move,
        Self::Ability,
        Self::Item,
        Self::Status,
        Self::Terrain,
        Self::Effect,
    ];

    /// Singular name used in messages and by `data.has(kind, id)` (`move`, `ability`, …).
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::Ability => "ability",
            Self::Item => "item",
            Self::Status => "status",
            Self::Terrain => "terrain",
            Self::Effect => "effect",
        }
    }

    /// Folder under `data/` holding this kind's RON files (`moves`, `abilities`, …).
    #[must_use]
    pub fn folder(self) -> &'static str {
        match self {
            Self::Move => "moves",
            Self::Ability => "abilities",
            Self::Item => "items",
            Self::Status => "statuses",
            Self::Terrain => "terrains",
            Self::Effect => "effects",
        }
    }

    /// Parses a singular [`name`](Self::name).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.name() == name)
    }
}

impl fmt::Display for ContentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// An ability (`data/abilities/*.ron`, optional `Ability(…)` name). Its behaviour is
/// written as battle hooks (§7.4); the record makes the id known so species and validation
/// can reference it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename = "Ability")]
pub struct AbilityDef {
    /// Content id.
    pub id: String,
    /// Display name (text key).
    #[serde(default)]
    pub name: String,
    /// Description (text key).
    #[serde(default)]
    pub description: String,
}

fn default_pocket() -> String {
    "items".into()
}

/// A bag item (`data/items/*.ron`, optional `Item(…)` name).
///
/// ```ron
/// Item(id: "potion", name: "text:items.potion", description: "text:items.potion_desc",
///      price: 300, pocket: "items")
/// ```
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename = "Item")]
pub struct ItemDef {
    /// Content id.
    pub id: String,
    /// Display name (text key).
    #[serde(default)]
    pub name: String,
    /// Description (text key).
    #[serde(default)]
    pub description: String,
    /// Shop price (`0` = cannot be bought).
    #[serde(default)]
    pub price: u32,
    /// Bag pocket (`items`, `balls`, `key_items`, …).
    #[serde(default = "default_pocket")]
    pub pocket: String,
}

fn default_true() -> bool {
    true
}

/// A custom terrain (§6.3). Tiles name it with the `terrain=` property; built-in terrains
/// (`grass`, `water`, `ice`, `ledge_*`, …) cannot be redefined. A Lua rule registered with
/// `world.register_terrain` takes precedence over these data rules.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename = "Terrain")]
pub struct TerrainDef {
    /// Terrain name as used by `terrain=`.
    pub id: String,
    /// `false` blocks every step onto the terrain.
    #[serde(default = "default_true")]
    pub passable: bool,
    /// Field state required to step onto it (`surf`, `plugin:state`).
    #[serde(default)]
    pub requires_field: Option<String>,
    /// Encounter table rolled on this terrain, if any.
    #[serde(default)]
    pub encounter: Option<String>,
}

/// A data effect primitive: a named effect whose behaviour a plugin implements in Lua
/// (`data.register_effect(def, fn)`).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename = "Effect")]
pub struct EffectDef {
    /// Content id.
    pub id: String,
    /// What the effect does (for tooling).
    #[serde(default)]
    pub description: String,
}

/// One record of any [`ContentKind`].
#[derive(Clone, Debug, PartialEq)]
pub enum ContentDef {
    /// A move.
    Move(MoveDef),
    /// An ability.
    Ability(AbilityDef),
    /// An item.
    Item(ItemDef),
    /// A status.
    Status(StatusDef),
    /// A terrain.
    Terrain(TerrainDef),
    /// A data effect primitive.
    Effect(EffectDef),
}

impl ContentDef {
    /// Its kind.
    #[must_use]
    pub fn kind(&self) -> ContentKind {
        match self {
            Self::Move(_) => ContentKind::Move,
            Self::Ability(_) => ContentKind::Ability,
            Self::Item(_) => ContentKind::Item,
            Self::Status(_) => ContentKind::Status,
            Self::Terrain(_) => ContentKind::Terrain,
            Self::Effect(_) => ContentKind::Effect,
        }
    }

    /// Its id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Move(d) => &d.id,
            Self::Ability(d) => &d.id,
            Self::Item(d) => &d.id,
            Self::Status(d) => &d.id,
            Self::Terrain(d) => &d.id,
            Self::Effect(d) => &d.id,
        }
    }

    /// Builds a record of `kind` from a dynamic value (a Lua table). Returns the record and
    /// its `override` flag. `context` names the caller in errors (`data.register_move`).
    pub fn from_value(kind: ContentKind, value: ContentValue, context: &str) -> Result<Record> {
        let mut value = value;
        let override_existing = match &mut value {
            ContentValue::Map(map) => match map.remove("override") {
                None | Some(ContentValue::Unit) => false,
                Some(ContentValue::Bool(b)) => b,
                Some(_) => {
                    return Err(DataError::InvalidField {
                        field: "override".into(),
                        context: context.into(),
                        message: "must be a boolean".into(),
                    });
                }
            },
            _ => {
                return Err(DataError::Parse {
                    file: context.into(),
                    message: format!("a {kind} must be a table"),
                });
            }
        };
        let parse_err = |e: ContentValueError| DataError::Parse {
            file: context.into(),
            message: e.to_string(),
        };
        let def = match kind {
            ContentKind::Move => Self::Move(MoveDef::deserialize(value).map_err(parse_err)?),
            ContentKind::Ability => {
                Self::Ability(AbilityDef::deserialize(value).map_err(parse_err)?)
            }
            ContentKind::Item => Self::Item(ItemDef::deserialize(value).map_err(parse_err)?),
            ContentKind::Status => Self::Status(StatusDef::deserialize(value).map_err(parse_err)?),
            ContentKind::Terrain => {
                Self::Terrain(TerrainDef::deserialize(value).map_err(parse_err)?)
            }
            ContentKind::Effect => Self::Effect(EffectDef::deserialize(value).map_err(parse_err)?),
        };
        Ok(Record {
            def,
            override_existing,
        })
    }

    /// Schema checks (stage 2, §8.2) beyond what the types enforce.
    pub fn validate(&self) -> Result<()> {
        let invalid = |field: &str, message: &str| DataError::InvalidField {
            field: field.into(),
            context: self.id().into(),
            message: message.into(),
        };
        match self {
            Self::Move(mv) => {
                if mv.type_id.is_empty() {
                    return Err(invalid("type", "must not be empty"));
                }
                if mv.accuracy > 100 {
                    return Err(invalid("accuracy", "must be 0..=100"));
                }
                for effect in &mv.effects {
                    match effect {
                        MoveEffect::ApplyStatus { chance, .. }
                        | MoveEffect::StatStage { chance, .. }
                            if *chance > 100 =>
                        {
                            return Err(invalid("effects", "chance must be 0..=100"));
                        }
                        MoveEffect::Heal { percent } if !(1..=100).contains(percent) => {
                            return Err(invalid("effects", "heal percent must be 1..=100"));
                        }
                        MoveEffect::MultiHit { min, max } if *min == 0 || max < min => {
                            return Err(invalid("effects", "multi_hit needs 1 <= min <= max"));
                        }
                        _ => {}
                    }
                }
                Ok(())
            }
            Self::Status(def) => def.validate(),
            Self::Terrain(def) => match def.requires_field.as_deref() {
                Some("") => Err(invalid("requires_field", "must not be empty")),
                _ => Ok(()),
            },
            Self::Ability(_) | Self::Item(_) | Self::Effect(_) => Ok(()),
        }
    }
}

/// A parsed record and whether it declared `override`.
#[derive(Clone, Debug, PartialEq)]
pub struct Record {
    /// The record.
    pub def: ContentDef,
    /// `override: true`: replace an existing entry with the same id.
    pub override_existing: bool,
}

/// Where a content entry came from.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContentSource {
    /// The game's own `data/` folder or scripts.
    Game,
    /// Plugin `id` (its `data/` folder or `data.register_*`).
    Plugin(String),
}

impl fmt::Display for ContentSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Game => f.write_str("the game"),
            Self::Plugin(id) => write!(f, "plugin `{id}`"),
        }
    }
}

/// One entry of the [`ContentDb`].
#[derive(Clone, Debug, PartialEq)]
pub struct ContentEntry {
    /// The record.
    pub def: ContentDef,
    /// Who defined it (the last override wins).
    pub source: ContentSource,
}

/// An override that replaced an existing entry (§9.4: overrides are logged).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentOverride {
    /// Kind of content.
    pub kind: ContentKind,
    /// Its id.
    pub id: String,
    /// Who defined the replaced entry.
    pub previous: ContentSource,
    /// Who replaced it.
    pub by: ContentSource,
}

impl fmt::Display for ContentOverride {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} `{}` from {} overridden by {}",
            self.kind, self.id, self.previous, self.by
        )
    }
}

/// The merged content database.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ContentDb {
    entries: BTreeMap<ContentKind, BTreeMap<String, ContentEntry>>,
    overrides: Vec<ContentOverride>,
}

/// `true` for a content id: `snake_case` or `plugin_id:snake_case`.
#[must_use]
pub fn is_content_id(id: &str) -> bool {
    is_action_name(id)
}

impl ContentDb {
    /// Empty database.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates and inserts `record` from `source`.
    ///
    /// Errors: invalid id or namespace, schema problems, or a duplicate id without
    /// `override`. With `override`, an existing entry is replaced and the override returned
    /// (and kept in [`overrides`](Self::overrides)); overriding an id that does not exist yet
    /// simply defines it.
    pub fn insert(
        &mut self,
        record: Record,
        source: ContentSource,
    ) -> Result<Option<ContentOverride>> {
        let Record {
            def,
            override_existing,
        } = record;
        let kind = def.kind();
        let id = def.id().to_owned();
        if !is_content_id(&id) {
            return Err(DataError::InvalidField {
                field: "id".into(),
                context: format!("{kind} `{id}`"),
                message: "must be snake_case or `plugin_id:snake_case`".into(),
            });
        }
        def.validate()?;
        let entries = self.entries.entry(kind).or_default();
        let existing = entries.get(&id).map(|e| e.source.clone());
        if existing.is_none() {
            check_namespace(kind, &id, &source)?;
        }
        match existing {
            Some(previous) if !override_existing => Err(DataError::DuplicateContent {
                kind: kind.name().into(),
                id,
                existing: previous.to_string(),
                by: source.to_string(),
            }),
            Some(previous) => {
                let ov = ContentOverride {
                    kind,
                    id: id.clone(),
                    previous,
                    by: source.clone(),
                };
                entries.insert(id, ContentEntry { def, source });
                self.overrides.push(ov.clone());
                Ok(Some(ov))
            }
            None => {
                entries.insert(id, ContentEntry { def, source });
                Ok(None)
            }
        }
    }

    /// The entry `kind`/`id`.
    #[must_use]
    pub fn get(&self, kind: ContentKind, id: &str) -> Option<&ContentEntry> {
        self.entries.get(&kind)?.get(id)
    }

    /// `true` if `kind`/`id` exists.
    #[must_use]
    pub fn contains(&self, kind: ContentKind, id: &str) -> bool {
        self.get(kind, id).is_some()
    }

    /// Ids of `kind`, sorted.
    #[must_use]
    pub fn ids(&self, kind: ContentKind) -> Vec<&str> {
        self.entries
            .get(&kind)
            .map(|m| m.keys().map(String::as_str).collect())
            .unwrap_or_default()
    }

    /// Number of entries of `kind`.
    #[must_use]
    pub fn len(&self, kind: ContentKind) -> usize {
        self.entries.get(&kind).map_or(0, BTreeMap::len)
    }

    /// `true` when nothing has been registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.values().all(BTreeMap::is_empty)
    }

    /// Every override applied so far, in order.
    #[must_use]
    pub fn overrides(&self) -> &[ContentOverride] {
        &self.overrides
    }

    /// Move `id`.
    #[must_use]
    pub fn move_def(&self, id: &str) -> Option<&MoveDef> {
        match &self.get(ContentKind::Move, id)?.def {
            ContentDef::Move(d) => Some(d),
            _ => None,
        }
    }

    /// Ability `id`.
    #[must_use]
    pub fn ability(&self, id: &str) -> Option<&AbilityDef> {
        match &self.get(ContentKind::Ability, id)?.def {
            ContentDef::Ability(d) => Some(d),
            _ => None,
        }
    }

    /// Item `id`.
    #[must_use]
    pub fn item(&self, id: &str) -> Option<&ItemDef> {
        match &self.get(ContentKind::Item, id)?.def {
            ContentDef::Item(d) => Some(d),
            _ => None,
        }
    }

    /// Status `id`.
    #[must_use]
    pub fn status(&self, id: &str) -> Option<&StatusDef> {
        match &self.get(ContentKind::Status, id)?.def {
            ContentDef::Status(d) => Some(d),
            _ => None,
        }
    }

    /// Terrain `id`.
    #[must_use]
    pub fn terrain(&self, id: &str) -> Option<&TerrainDef> {
        match &self.get(ContentKind::Terrain, id)?.def {
            ContentDef::Terrain(d) => Some(d),
            _ => None,
        }
    }

    /// Effect primitive `id`.
    #[must_use]
    pub fn effect(&self, id: &str) -> Option<&EffectDef> {
        match &self.get(ContentKind::Effect, id)?.def {
            ContentDef::Effect(d) => Some(d),
            _ => None,
        }
    }
}

/// A new id `ns:name` must use the source's own namespace; the game uses plain ids only.
fn check_namespace(kind: ContentKind, id: &str, source: &ContentSource) -> Result<()> {
    let Some((ns, _)) = id.split_once(':') else {
        return Ok(());
    };
    let allowed = matches!(source, ContentSource::Plugin(p) if p == ns) && is_plugin_id(ns);
    if allowed {
        Ok(())
    } else {
        Err(DataError::InvalidField {
            field: "id".into(),
            context: format!("{kind} `{id}`"),
            message: format!(
                "{source} cannot define ids in the `{ns}:` namespace (use its own namespace or `override: true` on an existing entry)"
            ),
        })
    }
}

fn ron_records<T: DeserializeOwned>(source: &str, file: &str) -> Result<Vec<(T, bool)>> {
    Ok(RonCodec::decode_records(source, file)?
        .into_iter()
        .map(|d| (d.value, d.override_existing))
        .collect())
}

/// Parses a content file of `kind`: one unnamed record `( id: "…", … )` or a list of them.
/// Any record may set `override: true`.
pub fn parse_content(kind: ContentKind, source: &str, file: &str) -> Result<Vec<Record>> {
    fn wrap<T>(records: Vec<(T, bool)>, f: impl Fn(T) -> ContentDef) -> Vec<Record> {
        records
            .into_iter()
            .map(|(d, override_existing)| Record {
                def: f(d),
                override_existing,
            })
            .collect()
    }
    Ok(match kind {
        ContentKind::Move => wrap(ron_records(source, file)?, ContentDef::Move),
        ContentKind::Ability => wrap(ron_records(source, file)?, ContentDef::Ability),
        ContentKind::Item => wrap(ron_records(source, file)?, ContentDef::Item),
        ContentKind::Status => wrap(ron_records(source, file)?, ContentDef::Status),
        ContentKind::Terrain => wrap(ron_records(source, file)?, ContentDef::Terrain),
        ContentKind::Effect => wrap(ron_records(source, file)?, ContentDef::Effect),
    })
}

#[cfg(test)]
mod tests;
