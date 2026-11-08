//! Party list reorder mode (field menu).

/// Swap-two mode: pick a member, then another, to swap.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PartyReorderMode {
    /// Browse only.
    #[default]
    Browse,
    /// First member selected for a swap.
    SelectFirst(usize),
}

/// Party reorder UI state.
#[derive(Clone, Debug, Default)]
pub struct PartyReorderSession {
    mode: PartyReorderMode,
}

impl PartyReorderSession {
    /// Fresh session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Current mode.
    #[must_use]
    pub fn mode(&self) -> PartyReorderMode {
        self.mode
    }

    /// Toggles reorder mode; clears an in-progress selection.
    pub fn toggle_reorder(&mut self) {
        self.mode = match self.mode {
            PartyReorderMode::Browse => PartyReorderMode::SelectFirst(0),
            PartyReorderMode::SelectFirst(_) => PartyReorderMode::Browse,
        };
    }

    /// Confirm on row `index`. Returns `Some((from, to))` when a swap should happen.
    pub fn confirm(&mut self, index: usize) -> Option<(usize, usize)> {
        match self.mode {
            PartyReorderMode::Browse => None,
            PartyReorderMode::SelectFirst(first) => {
                if first == index {
                    None
                } else {
                    self.mode = PartyReorderMode::Browse;
                    Some((first, index))
                }
            }
        }
    }

    /// Moves the highlight in reorder mode (first selection).
    pub fn set_first(&mut self, index: usize) {
        if matches!(self.mode, PartyReorderMode::SelectFirst(_)) {
            self.mode = PartyReorderMode::SelectFirst(index);
        }
    }

    /// Cancels reorder mode.
    pub fn cancel(&mut self) {
        self.mode = PartyReorderMode::Browse;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swap_two_members() {
        let mut s = PartyReorderSession::new();
        s.toggle_reorder();
        s.set_first(0);
        assert_eq!(s.confirm(2), Some((0, 2)));
        assert_eq!(s.mode(), PartyReorderMode::Browse);
    }
}
