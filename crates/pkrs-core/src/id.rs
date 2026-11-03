//! Content identifiers: validated strings interned into typed handles.

use core::fmt;
use core::hash::Hash;
use core::marker::PhantomData;
use std::collections::HashMap;

use crate::error::{CoreError, Result};

/// Validates a content id string (species, moves, maps, …).
///
/// Rules: non-empty, ASCII lowercase letter first, then lowercase letters, digits or `_`.
pub fn validate_id(raw: &str) -> Result<()> {
    let Some(first) = raw.chars().next() else {
        return Err(CoreError::InvalidId(raw.to_owned()));
    };
    if !first.is_ascii_lowercase() {
        return Err(CoreError::InvalidId(raw.to_owned()));
    }
    if !raw
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
    {
        return Err(CoreError::InvalidId(raw.to_owned()));
    }
    Ok(())
}

/// Marker for [`TypedId`] kinds (species, moves, …).
pub trait IdKind: Copy + Eq + Ord + Hash + fmt::Debug + 'static {}

/// A typed, interned content id (runtime handle; persist author-facing strings in saves).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TypedId<K: IdKind> {
    raw: u32,
    _kind: PhantomData<K>,
}

impl<K: IdKind> TypedId<K> {
    /// Returns the dense index for this id kind in the current process (not stable across reloads).
    pub fn index(self) -> u32 {
        self.raw
    }
}

impl<K: IdKind> fmt::Debug for TypedId<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("TypedId")
            .field(&self.raw)
            .field(&PhantomData::<K>)
            .finish()
    }
}

/// Example kind tags; other crates may define their own zero-sized markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SpeciesKind {}

impl IdKind for SpeciesKind {}

/// Example kind tags; other crates may define their own zero-sized markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MoveKind {}

impl IdKind for MoveKind {}

/// Maps author-facing id strings to dense [`TypedId`] handles for one id kind.
#[derive(Clone, Debug)]
pub struct IdInterner<K: IdKind> {
    by_str: HashMap<String, u32>,
    strings: Vec<String>,
    _kind: PhantomData<K>,
}

impl<K: IdKind> Default for IdInterner<K> {
    fn default() -> Self {
        Self {
            by_str: HashMap::new(),
            strings: Vec::new(),
            _kind: PhantomData,
        }
    }
}

impl<K: IdKind> IdInterner<K> {
    /// Creates an empty interner.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the handle for `raw`, interning it on first use.
    pub fn intern(&mut self, raw: &str) -> Result<TypedId<K>> {
        validate_id(raw)?;
        if let Some(&index) = self.by_str.get(raw) {
            return Ok(TypedId {
                raw: index,
                _kind: PhantomData,
            });
        }
        let index =
            u32::try_from(self.strings.len()).map_err(|_| CoreError::IdInternerExhausted)?;
        self.by_str.insert(raw.to_owned(), index);
        self.strings.push(raw.to_owned());
        Ok(TypedId {
            raw: index,
            _kind: PhantomData,
        })
    }

    /// Resolves a handle back to the author-facing string.
    pub fn resolve(&self, id: TypedId<K>) -> Option<&str> {
        self.strings.get(id.raw as usize).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn validate_id_rejects_empty_and_uppercase() {
        assert!(validate_id("").is_err());
        assert!(validate_id("Ember").is_err());
        assert!(validate_id("1bad").is_err());
        assert!(validate_id("ember").is_ok());
        assert!(validate_id("route_1").is_ok());
    }

    #[test]
    fn interner_is_stable_per_kind() {
        let mut interner = IdInterner::<SpeciesKind>::new();
        let a: TypedId<SpeciesKind> = interner.intern("emberkit").expect("valid");
        let b: TypedId<SpeciesKind> = interner.intern("emberkit").expect("valid");
        assert_eq!(a, b);
        assert_eq!(interner.resolve(a), Some("emberkit"));
    }

    #[test]
    fn interners_do_not_share_namespaces() {
        let mut species = IdInterner::<SpeciesKind>::new();
        let mut moves = IdInterner::<MoveKind>::new();
        let s = species.intern("ember").expect("valid");
        let m = moves.intern("ember").expect("valid");
        assert_eq!(s.index(), 0);
        assert_eq!(m.index(), 0);
        assert_eq!(species.resolve(s), Some("ember"));
        assert_eq!(moves.resolve(m), Some("ember"));
    }

    #[test]
    fn typed_id_usable_in_hash_set() {
        let mut interner = IdInterner::<SpeciesKind>::new();
        let id = interner.intern("scratch").expect("valid");
        let set: HashSet<TypedId<SpeciesKind>> = HashSet::from([id]);
        assert!(set.contains(&id));
    }
}
