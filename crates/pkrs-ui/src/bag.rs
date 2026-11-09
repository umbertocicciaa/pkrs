//! Bag list sorting and pocket filtering (field menu).

use std::collections::BTreeMap;

/// One row in the bag list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BagEntry {
    /// Item id.
    pub id: String,
    /// Count in the bag.
    pub count: u32,
    /// Pocket name from content (`items`, `balls`, `key_items`, …).
    pub pocket: String,
}

/// Sorts and filters bag rows for the field menu.
#[derive(Clone, Debug)]
pub struct BagList {
    entries: Vec<BagEntry>,
    pocket: Option<String>,
}

impl BagList {
    /// Builds a list from `bag` counts; unknown items use pocket `"items"`.
    pub fn new(bag: &BTreeMap<String, u32>, pocket_of: impl Fn(&str) -> String) -> Self {
        let mut entries: Vec<BagEntry> = bag
            .iter()
            .filter(|(_, c)| **c > 0)
            .map(|(id, count)| BagEntry {
                id: id.clone(),
                count: *count,
                pocket: pocket_of(id),
            })
            .collect();
        entries.sort_by(|a, b| a.id.cmp(&b.id));
        Self {
            entries,
            pocket: None,
        }
    }

    /// When set, only entries in this pocket are shown.
    pub fn set_pocket_filter(&mut self, pocket: Option<String>) {
        self.pocket = pocket;
    }

    /// Active pocket filter.
    #[must_use]
    pub fn pocket_filter(&self) -> Option<&str> {
        self.pocket.as_deref()
    }

    /// Rows visible after filtering, in id order.
    #[must_use]
    pub fn visible(&self) -> Vec<&BagEntry> {
        self.entries
            .iter()
            .filter(|e| {
                self.pocket
                    .as_ref()
                    .is_none_or(|p| e.pocket.as_str() == p.as_str())
            })
            .collect()
    }

    /// Distinct pockets present in the bag, sorted.
    #[must_use]
    pub fn pockets(&self) -> Vec<String> {
        let mut p: Vec<String> = self.entries.iter().map(|e| e.pocket.clone()).collect();
        p.sort();
        p.dedup();
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_by_id_and_filters_pocket() {
        let bag = BTreeMap::from([
            ("zinc".into(), 1),
            ("potion".into(), 3),
            ("capture_orb".into(), 2),
        ]);
        let pocket = |id: &str| match id {
            "capture_orb" => "balls".into(),
            "lighthouse_key" => "key_items".into(),
            _ => "items".into(),
        };
        let mut list = BagList::new(&bag, pocket);
        assert_eq!(
            list.visible()
                .iter()
                .map(|e| e.id.as_str())
                .collect::<Vec<_>>(),
            ["capture_orb", "potion", "zinc"]
        );
        list.set_pocket_filter(Some("balls".into()));
        assert_eq!(list.visible().len(), 1);
        assert_eq!(list.visible()[0].id, "capture_orb");
    }
}
