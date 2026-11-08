//! Id interning (§8.1): string ids in the files become dense `u32` [`Handle`]s after
//! loading, one id space per record kind ([`Table`]).
//!
//! Handles are indices in load order (files in name order, records in file order), so the
//! same data always gives the same handles. They are runtime handles: saves keep the
//! author-facing string ids.

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;
use std::collections::HashMap;

use crate::battle_rules::StatusDef;
use crate::content::{AbilityDef, EffectDef, ItemDef, TerrainDef};
use crate::encounter::EncounterTable;
use crate::error::{DataError, Result};
use crate::move_def::MoveDef;
use crate::species::SpeciesDef;
use crate::trainer::{AiProfile, TrainerDef};

/// A record kind with a string id.
pub trait Keyed {
    /// Kind name used in messages (`species`, `move`, …).
    const KIND: &'static str;

    /// The record's id.
    fn key(&self) -> &str;
}

macro_rules! keyed {
    ($($ty:ty => $kind:literal),* $(,)?) => {
        $(impl Keyed for $ty {
            const KIND: &'static str = $kind;

            fn key(&self) -> &str {
                &self.id
            }
        })*
    };
}

keyed! {
    SpeciesDef => "species",
    MoveDef => "move",
    AbilityDef => "ability",
    ItemDef => "item",
    StatusDef => "status",
    TerrainDef => "terrain",
    EffectDef => "effect",
    TrainerDef => "trainer",
    AiProfile => "ai profile",
    EncounterTable => "encounter table",
}

/// Interned handle of a record of type `T` in a [`Table`].
pub struct Handle<T> {
    index: u32,
    _kind: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    fn new(index: u32) -> Self {
        Self {
            index,
            _kind: PhantomData,
        }
    }

    /// The dense index (load order).
    #[must_use]
    pub fn index(self) -> u32 {
        self.index
    }

    fn slot(self) -> usize {
        self.index as usize
    }
}

impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}

impl<T> Eq for Handle<T> {}

impl<T> PartialOrd for Handle<T> {
    fn partial_cmp(&self, other: &Self) -> Option<core::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for Handle<T> {
    fn cmp(&self, other: &Self) -> core::cmp::Ordering {
        self.index.cmp(&other.index)
    }
}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Handle({})", self.index)
    }
}

/// All records of one kind, interned: id → [`Handle`] → record.
#[derive(Clone, Debug, PartialEq)]
pub struct Table<T> {
    records: Vec<T>,
    files: Vec<String>,
    index: HashMap<String, u32>,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            records: Vec::new(),
            files: Vec::new(),
            index: HashMap::new(),
        }
    }
}

impl<T: Keyed> Table<T> {
    /// Empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `record` from `file`. A second record with the same id is an error
    /// ([`DataError::DuplicateId`], naming both files) unless `replace` is set, in which case
    /// it replaces the first one and keeps its handle.
    pub fn insert(&mut self, record: T, file: &str, replace: bool) -> Result<Handle<T>> {
        if let Some(&index) = self.index.get(record.key()) {
            let slot = index as usize;
            if !replace {
                return Err(DataError::DuplicateId {
                    kind: T::KIND.into(),
                    id: record.key().to_owned(),
                    file: file.into(),
                    first: self.files.get(slot).cloned().unwrap_or_default(),
                });
            }
            if let (Some(r), Some(f)) = (self.records.get_mut(slot), self.files.get_mut(slot)) {
                *r = record;
                *f = file.to_owned();
            }
            return Ok(Handle::new(index));
        }
        let index = u32::try_from(self.records.len()).map_err(|_| DataError::InvalidField {
            field: "id".into(),
            context: file.into(),
            message: format!("too many {} records", T::KIND),
        })?;
        self.index.insert(record.key().to_owned(), index);
        self.records.push(record);
        self.files.push(file.to_owned());
        Ok(Handle::new(index))
    }

    /// The handle of `id`.
    #[must_use]
    pub fn handle(&self, id: &str) -> Option<Handle<T>> {
        self.index.get(id).copied().map(Handle::new)
    }

    /// The record of `handle` (`None` for a handle of another table).
    #[must_use]
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.records.get(handle.slot())
    }

    /// The record with id `id`.
    #[must_use]
    pub fn by_id(&self, id: &str) -> Option<&T> {
        self.get(self.handle(id)?)
    }

    /// `true` if `id` exists.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.index.contains_key(id)
    }

    /// The id of `handle`.
    #[must_use]
    pub fn id(&self, handle: Handle<T>) -> Option<&str> {
        self.get(handle).map(Keyed::key)
    }

    /// The file (or content source) that defined `handle`.
    #[must_use]
    pub fn file(&self, handle: Handle<T>) -> Option<&str> {
        self.files.get(handle.slot()).map(String::as_str)
    }

    /// Number of records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Every record with its handle, in handle (load) order.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        (0u32..)
            .zip(self.records.iter())
            .map(|(i, r)| (Handle::new(i), r))
    }

    /// Every record, in handle order.
    pub fn records(&self) -> &[T] {
        &self.records
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;

    fn item(id: &str, price: u32) -> ItemDef {
        ItemDef {
            id: id.into(),
            name: String::new(),
            description: String::new(),
            price,
            pocket: "items".into(),
        }
    }

    #[test]
    fn interning_is_dense_stable_and_per_kind() {
        let mut items = Table::new();
        let a = items.insert(item("potion", 1), "a.ron", false).unwrap();
        let b = items.insert(item("ether", 2), "a.ron", false).unwrap();
        assert_eq!((a.index(), b.index()), (0, 1));
        assert_eq!(items.handle("ether"), Some(b));
        assert_eq!(items.id(a), Some("potion"));
        assert_eq!(items.get(b).map(|i| i.price), Some(2));
        assert_eq!(items.handle("missing"), None);
        let ids: Vec<&str> = items.iter().map(|(_, i)| i.id.as_str()).collect();
        assert_eq!(ids, ["potion", "ether"]);

        let mut moves: Table<MoveDef> = Table::new();
        let m = moves
            .insert(
                MoveDef {
                    id: "potion".into(),
                    ..MoveDef::default()
                },
                "m.ron",
                false,
            )
            .unwrap();
        assert_eq!(m.index(), 0, "another kind has its own id space");
        assert_eq!(moves.get(Handle::new(5)), None);
    }

    #[test]
    fn duplicates_name_both_files_and_replace_keeps_the_handle() {
        let mut items = Table::new();
        let a = items
            .insert(item("potion", 1), "data/items/a.ron", false)
            .unwrap();
        let err = items
            .insert(item("potion", 2), "data/items/b.ron", false)
            .unwrap_err();
        assert_eq!(
            err.to_string(),
            "duplicate item `potion` in `data/items/b.ron` (already defined in `data/items/a.ron`)"
        );
        let b = items
            .insert(item("potion", 3), "data/items/b.ron", true)
            .unwrap();
        assert_eq!(a, b);
        assert_eq!(items.len(), 1);
        assert_eq!(items.by_id("potion").map(|i| i.price), Some(3));
        assert_eq!(items.file(a), Some("data/items/b.ron"));
    }
}
