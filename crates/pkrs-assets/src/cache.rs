//! Load-on-first-request cache with reference counting (§13).

use std::collections::HashMap;

use crate::error::{AssetsError, Result};
use crate::handle::{Handle, SlotMap};
use crate::path::AssetPath;

/// A type that can be decoded from the bytes of one file.
///
/// Decoders live with the subsystem that owns the type (textures in `pkrs-render`, sounds in
/// `pkrs-audio`, …); this crate only provides [`RawBytes`] and [`Text`].
pub trait Asset: Sized + 'static {
    /// Decodes the file at `path`. Return [`AssetsError::Decode`] on malformed input.
    fn decode(bytes: Vec<u8>, path: &AssetPath) -> Result<Self>;
}

/// The undecoded file contents.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawBytes(pub Vec<u8>);

impl Asset for RawBytes {
    fn decode(bytes: Vec<u8>, _path: &AssetPath) -> Result<Self> {
        Ok(Self(bytes))
    }
}

/// A UTF-8 text file (RON, Lua, Tiled XML, …). A leading byte-order mark is stripped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text(pub String);

impl Asset for Text {
    fn decode(bytes: Vec<u8>, path: &AssetPath) -> Result<Self> {
        let mut text = String::from_utf8(bytes).map_err(|e| AssetsError::Decode {
            path: path.to_string(),
            message: format!("not valid UTF-8: {e}"),
        })?;
        if text.starts_with('\u{feff}') {
            text.drain(..'\u{feff}'.len_utf8());
        }
        Ok(Self(text))
    }
}

#[derive(Debug)]
struct Entry<T> {
    path: AssetPath,
    value: T,
    refs: u32,
}

/// Cache for one asset type: each path is loaded once and shared through a [`Handle<T>`].
///
/// Every successful [`get_or_load`](Self::get_or_load) or [`acquire`](Self::acquire) adds a
/// reference; [`release`](Self::release) drops one. Entries with no references stay cached
/// until [`evict_unused`](Self::evict_unused) runs (e.g. on map change), so going back and
/// forth between two maps does not reload shared assets.
#[derive(Debug)]
pub struct AssetStore<T> {
    entries: SlotMap<Entry<T>>,
    by_path: HashMap<AssetPath, Handle<Entry<T>>>,
}

impl<T> Default for AssetStore<T> {
    fn default() -> Self {
        Self {
            entries: SlotMap::new(),
            by_path: HashMap::new(),
        }
    }
}

impl<T> AssetStore<T> {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the cached handle for `path` (adding a reference), or runs `load` on a miss.
    pub fn get_or_load(
        &mut self,
        path: &AssetPath,
        load: impl FnOnce(&AssetPath) -> Result<T>,
    ) -> Result<Handle<T>> {
        if let Some(&inner) = self.by_path.get(path) {
            if let Some(entry) = self.entries.get_mut(inner) {
                entry.refs = entry.refs.saturating_add(1);
                return Ok(inner.cast());
            }
        }
        let value = load(path)?;
        let inner = self
            .entries
            .insert(Entry {
                path: path.clone(),
                value,
                refs: 1,
            })
            .ok_or_else(|| AssetsError::Io {
                path: path.to_string(),
                message: "asset store is full".to_owned(),
            })?;
        self.by_path.insert(path.clone(), inner);
        Ok(inner.cast())
    }

    /// The cached handle for `path`, without changing its reference count.
    pub fn lookup(&self, path: &AssetPath) -> Option<Handle<T>> {
        self.by_path.get(path).map(|h| h.cast())
    }

    /// The asset, if `handle` is still cached.
    pub fn get(&self, handle: Handle<T>) -> Option<&T> {
        self.entries.get(handle.cast()).map(|e| &e.value)
    }

    /// The path an asset was loaded from.
    pub fn path_of(&self, handle: Handle<T>) -> Option<&AssetPath> {
        self.entries.get(handle.cast()).map(|e| &e.path)
    }

    /// Current reference count.
    pub fn refs(&self, handle: Handle<T>) -> Option<u32> {
        self.entries.get(handle.cast()).map(|e| e.refs)
    }

    /// Adds a reference to a cached asset.
    pub fn acquire(&mut self, handle: Handle<T>) -> Result<()> {
        let entry = self
            .entries
            .get_mut(handle.cast())
            .ok_or(AssetsError::StaleHandle)?;
        entry.refs = entry.refs.saturating_add(1);
        Ok(())
    }

    /// Drops a reference; returns the remaining count. The asset stays cached at zero.
    pub fn release(&mut self, handle: Handle<T>) -> Result<u32> {
        let entry = self
            .entries
            .get_mut(handle.cast())
            .ok_or(AssetsError::StaleHandle)?;
        entry.refs = entry.refs.saturating_sub(1);
        Ok(entry.refs)
    }

    /// Drops every asset with no references; returns how many were evicted.
    /// Handles to evicted assets become stale.
    pub fn evict_unused(&mut self) -> usize {
        let unused: Vec<_> = self
            .entries
            .iter()
            .filter(|(_, e)| e.refs == 0)
            .map(|(h, _)| h)
            .collect();
        for handle in &unused {
            if let Some(entry) = self.entries.remove(*handle) {
                self.by_path.remove(&entry.path);
            }
        }
        unused.len()
    }

    /// Number of cached assets.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if nothing is cached.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;

    fn path(raw: &str) -> AssetPath {
        AssetPath::parse(raw).expect("valid")
    }

    #[test]
    fn second_request_is_a_cache_hit() {
        let loads = Cell::new(0);
        let mut store = AssetStore::new();
        let load = |p: &AssetPath| {
            loads.set(loads.get() + 1);
            Ok(p.to_string())
        };
        let a = store.get_or_load(&path("a.ron"), load).expect("loads");
        let again = store.get_or_load(&path("./a.ron"), load).expect("hit");
        assert_eq!(a, again);
        assert_eq!(loads.get(), 1);
        assert_eq!(store.refs(a), Some(2));
        assert_eq!(store.get(a).map(String::as_str), Some("a.ron"));
        assert_eq!(store.path_of(a), Some(&path("a.ron")));
        assert_eq!(store.lookup(&path("a.ron")), Some(a));
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn failed_load_caches_nothing() {
        let mut store: AssetStore<u8> = AssetStore::new();
        let err = store.get_or_load(&path("x"), |p| Err(AssetsError::NotFound(p.to_string())));
        assert!(err.is_err());
        assert!(store.is_empty());
        assert_eq!(store.lookup(&path("x")), None);
    }

    #[test]
    fn refcount_controls_eviction() {
        let mut store = AssetStore::new();
        let shared = store.get_or_load(&path("shared"), |_| Ok(1)).expect("ok");
        let only_here = store
            .get_or_load(&path("only_here"), |_| Ok(2))
            .expect("ok");
        store.acquire(shared).expect("live");
        assert_eq!(store.refs(shared), Some(2));

        // Leaving the map releases one reference to each.
        assert_eq!(store.release(shared).expect("live"), 1);
        assert_eq!(store.release(only_here).expect("live"), 0);
        assert_eq!(store.release(only_here).expect("live"), 0, "saturates");
        assert_eq!(
            store.get(only_here),
            Some(&2),
            "zero refs stays cached until eviction"
        );

        assert_eq!(store.evict_unused(), 1);
        assert_eq!(store.get(only_here), None);
        assert_eq!(store.lookup(&path("only_here")), None);
        assert!(matches!(
            store.release(only_here),
            Err(AssetsError::StaleHandle)
        ));
        assert!(matches!(
            store.acquire(only_here),
            Err(AssetsError::StaleHandle)
        ));
        assert_eq!(store.get(shared), Some(&1));
        assert_eq!(store.evict_unused(), 0);
    }

    #[test]
    fn reload_after_eviction_gets_a_fresh_handle() {
        let mut store = AssetStore::new();
        let old = store.get_or_load(&path("a"), |_| Ok("v1")).expect("ok");
        store.release(old).expect("live");
        store.evict_unused();
        let new = store.get_or_load(&path("a"), |_| Ok("v2")).expect("ok");
        assert_eq!(new.index(), old.index(), "slot reused");
        assert_ne!(new, old);
        assert_eq!(store.get(old), None);
        assert_eq!(store.get(new), Some(&"v2"));
    }

    #[test]
    fn text_strips_bom_and_rejects_invalid_utf8() {
        let p = path("data/types.ron");
        let text = Text::decode(b"\xEF\xBB\xBF(fire)".to_vec(), &p).expect("utf-8");
        assert_eq!(text.0, "(fire)");
        assert!(matches!(
            Text::decode(vec![0xff, 0xfe, 0x00], &p),
            Err(AssetsError::Decode { .. })
        ));
        assert_eq!(RawBytes::decode(vec![1, 2], &p).expect("raw").0, [1, 2]);
    }
}
