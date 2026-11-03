//! Typed generational handles and the slot map that issues them (§13).

use core::fmt;
use core::hash::{Hash, Hasher};
use core::marker::PhantomData;

/// A typed handle to a `T` stored in a [`SlotMap<T>`]: a `u32` slot index plus a generation.
///
/// Handles are `Copy` and cheap to store in data structs. When a slot is freed its generation
/// is bumped, so an old handle to a reused slot is detected as stale instead of aliasing the
/// new value.
pub struct Handle<T> {
    index: u32,
    generation: u32,
    _type: PhantomData<fn() -> T>,
}

impl<T> Handle<T> {
    const fn new(index: u32, generation: u32) -> Self {
        Self {
            index,
            generation,
            _type: PhantomData,
        }
    }

    /// Slot index (dense, reused after removal).
    pub fn index(self) -> u32 {
        self.index
    }

    /// Generation of the slot when this handle was issued.
    pub fn generation(self) -> u32 {
        self.generation
    }

    /// A handle that was never issued by any map (tests only).
    #[cfg(test)]
    pub(crate) const fn default_for_tests() -> Self {
        Self::new(u32::MAX, u32::MAX)
    }

    /// Re-types the handle (stores keep `Handle<Entry<T>>` internally but hand out `Handle<T>`).
    pub(crate) const fn cast<U>(self) -> Handle<U> {
        Handle::new(self.index, self.generation)
    }
}

// Manual impls: derives would wrongly require `T: Clone`/`T: Eq`/… .
impl<T> Clone for Handle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for Handle<T> {}

impl<T> PartialEq for Handle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index && self.generation == other.generation
    }
}

impl<T> Eq for Handle<T> {}

impl<T> Hash for Handle<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.index.hash(state);
        self.generation.hash(state);
    }
}

impl<T> fmt::Debug for Handle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Handle<{}>({}v{})",
            core::any::type_name::<T>(),
            self.index,
            self.generation
        )
    }
}

#[derive(Debug, Clone)]
struct Slot<T> {
    generation: u32,
    value: Option<T>,
}

/// A generational slot map: O(1) insert, lookup and removal by [`Handle`].
#[derive(Debug, Clone)]
pub struct SlotMap<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }
}

impl<T> SlotMap<T> {
    /// Creates an empty map.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores `value`, reusing a freed slot if possible.
    ///
    /// Returns `None` only if all `u32::MAX` slots are in use.
    pub fn insert(&mut self, value: T) -> Option<Handle<T>> {
        if let Some(index) = self.free.pop() {
            let slot = self.slots.get_mut(index as usize)?;
            slot.value = Some(value);
            self.len += 1;
            return Some(Handle::new(index, slot.generation));
        }
        let index = u32::try_from(self.slots.len()).ok()?;
        self.slots.push(Slot {
            generation: 0,
            value: Some(value),
        });
        self.len += 1;
        Some(Handle::new(index, 0))
    }

    /// Returns the value if `handle` is still live.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.slots
            .get(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.value.as_ref())
    }

    /// Mutable access if `handle` is still live.
    pub fn get_mut(&mut self, handle: Handle<T>) -> Option<&mut T> {
        self.slots
            .get_mut(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)
            .and_then(|slot| slot.value.as_mut())
    }

    /// Returns `true` if `handle` refers to a live value.
    pub fn contains(&self, handle: Handle<T>) -> bool {
        self.get(handle).is_some()
    }

    /// Removes and returns the value; the handle (and copies of it) become stale.
    pub fn remove(&mut self, handle: Handle<T>) -> Option<T> {
        let slot = self
            .slots
            .get_mut(handle.index as usize)
            .filter(|slot| slot.generation == handle.generation)?;
        let value = slot.value.take()?;
        self.len -= 1;
        // A slot whose generation would wrap is retired rather than risk aliasing.
        if let Some(next) = slot.generation.checked_add(1) {
            slot.generation = next;
            self.free.push(handle.index);
        }
        Some(value)
    }

    /// Number of live values.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if there are no live values.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Iterates over live `(handle, value)` pairs in slot order.
    pub fn iter(&self) -> impl Iterator<Item = (Handle<T>, &T)> {
        self.slots.iter().enumerate().filter_map(|(i, slot)| {
            let index = u32::try_from(i).ok()?;
            slot.value
                .as_ref()
                .map(|v| (Handle::new(index, slot.generation), v))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn insert_get_remove() {
        let mut map = SlotMap::new();
        let a = map.insert("a").expect("room");
        let b = map.insert("b").expect("room");
        assert_eq!(map.get(a), Some(&"a"));
        assert_eq!(map.len(), 2);
        if let Some(v) = map.get_mut(b) {
            *v = "bb";
        }
        assert_eq!(map.remove(b), Some("bb"));
        assert_eq!(map.remove(b), None, "double remove is a no-op");
        assert!(!map.contains(b));
        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());
    }

    #[test]
    fn freed_slot_is_reused_with_a_new_generation() {
        let mut map = SlotMap::new();
        let old = map.insert(1).expect("room");
        map.remove(old);
        let new = map.insert(2).expect("room");
        assert_eq!(new.index(), old.index(), "slot reused");
        assert_eq!(new.generation(), old.generation() + 1);
        assert_ne!(new, old);
        assert_eq!(
            map.get(old),
            None,
            "stale handle does not alias the new value"
        );
        assert_eq!(map.get(new), Some(&2));
    }

    #[test]
    fn exhausted_generation_retires_the_slot() {
        let mut map = SlotMap::new();
        let h = map.insert(0).expect("room");
        if let Some(slot) = map.slots.get_mut(0) {
            slot.generation = u32::MAX;
        }
        let maxed = Handle::new(h.index(), u32::MAX);
        assert_eq!(map.remove(maxed), Some(0));
        let next = map.insert(1).expect("room");
        assert_eq!(next.index(), 1, "retired slot is not reused");
    }

    #[test]
    fn iter_yields_live_values_only() {
        let mut map = SlotMap::new();
        let a = map.insert('a').expect("room");
        let b = map.insert('b').expect("room");
        map.remove(a);
        let live: Vec<_> = map.iter().collect();
        assert_eq!(live, [(b, &'b')]);
    }

    #[test]
    fn handles_are_copy_hash_and_debug_without_bounds_on_t() {
        struct NotClone;
        let mut map = SlotMap::new();
        let h = map.insert(NotClone).expect("room");
        let copy = h;
        let set: HashSet<Handle<NotClone>> = HashSet::from([h, copy]);
        assert_eq!(set.len(), 1);
        assert!(format!("{h:?}").ends_with("(0v0)"));
    }
}
