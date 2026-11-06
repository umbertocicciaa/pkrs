//! Player input lock held by running cutscenes (§9.3).

use std::collections::BTreeSet;

use pkrs_core::Direction;

/// Who holds a [`PlayerLock`] (e.g. a script task id).
pub type LockHolder = u64;

/// Locks player movement while one or more holders (usually interact/trigger script tasks,
/// §9.3) are running. The player is locked while any holder remains, so overlapping cutscenes
/// cannot unlock each other.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerLock {
    holders: BTreeSet<LockHolder>,
}

impl PlayerLock {
    /// Unlocked.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `holder`. Returns `false` if it already held the lock.
    pub fn lock(&mut self, holder: LockHolder) -> bool {
        self.holders.insert(holder)
    }

    /// Removes `holder`. Returns `false` if it did not hold the lock.
    pub fn unlock(&mut self, holder: LockHolder) -> bool {
        self.holders.remove(&holder)
    }

    /// Releases every holder.
    pub fn clear(&mut self) {
        self.holders.clear();
    }

    /// `true` while any holder remains.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        !self.holders.is_empty()
    }

    /// `true` if `holder` holds the lock.
    #[must_use]
    pub fn is_held_by(&self, holder: LockHolder) -> bool {
        self.holders.contains(&holder)
    }

    /// Number of holders.
    #[must_use]
    pub fn holder_count(&self) -> usize {
        self.holders.len()
    }

    /// Filters a movement request: `None` while locked, `dir` otherwise. Pass the result to
    /// [`GridStepper::tick`](super::GridStepper::tick) for the player.
    #[must_use]
    pub fn gate(&self, dir: Option<Direction>) -> Option<Direction> {
        if self.is_locked() { None } else { dir }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locked_while_any_holder_remains() {
        let mut lock = PlayerLock::new();
        assert!(!lock.is_locked());
        assert!(lock.lock(1));
        assert!(!lock.lock(1));
        assert!(lock.lock(2));
        assert!(lock.unlock(1));
        assert!(lock.is_locked());
        assert!(!lock.unlock(1));
        assert!(lock.unlock(2));
        assert!(!lock.is_locked());
    }

    #[test]
    fn gate_drops_input_while_locked() {
        let mut lock = PlayerLock::new();
        assert_eq!(lock.gate(Some(Direction::Up)), Some(Direction::Up));
        lock.lock(7);
        assert_eq!(lock.gate(Some(Direction::Up)), None);
        lock.clear();
        assert_eq!(lock.gate(Some(Direction::Left)), Some(Direction::Left));
    }
}
