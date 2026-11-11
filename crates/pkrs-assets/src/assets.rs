//! The `Assets` facade (§4.5): mount table + typed, reference-counted caches.

use core::any::{Any, TypeId};
use std::collections::{BTreeMap, HashMap};

use crate::cache::{Asset, AssetStore};
use crate::error::{AssetsError, Result};
use crate::handle::Handle;
use crate::path::{AssetPath, VfsPath, is_mount_id};
use crate::vfs::{EntryKind, SubVfs, Vfs};

/// One entry returned by [`Assets::list`].
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetEntry {
    /// Full path, including the mount prefix.
    pub path: AssetPath,
    /// File or directory.
    pub kind: EntryKind,
}

/// Type-erased view of an [`AssetStore<T>`] so one facade can hold stores of many types.
trait ErasedStore {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn evict_unused(&mut self) -> usize;
    fn len(&self) -> usize;
}

impl<T: 'static> ErasedStore for AssetStore<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn evict_unused(&mut self) -> usize {
        AssetStore::evict_unused(self)
    }

    fn len(&self) -> usize {
        AssetStore::len(self)
    }
}

/// Game-facing asset service: reads through the mount table and caches decoded assets.
///
/// The project is the root mount; plugins are mounted under `plugin_id:` prefixes (§13).
/// The same calls work whether the mounts are a [`FolderVfs`](crate::FolderVfs),
/// a [`MemoryVfs`](crate::MemoryVfs) or a [`PakVfs`](crate::PakVfs) archive.
pub struct Assets {
    root: Box<dyn Vfs>,
    mounts: BTreeMap<String, Box<dyn Vfs>>,
    stores: HashMap<TypeId, Box<dyn ErasedStore>>,
}

impl core::fmt::Debug for Assets {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Assets")
            .field("mounts", &self.mounts.keys().collect::<Vec<_>>())
            .field("cached", &self.cached_count())
            .finish_non_exhaustive()
    }
}

impl Assets {
    /// Creates the facade over the project's root file system.
    pub fn new(root: impl Vfs + 'static) -> Self {
        Self {
            root: Box::new(root),
            mounts: BTreeMap::new(),
            stores: HashMap::new(),
        }
    }

    /// Mounts `vfs` under `prefix:` (a plugin id).
    pub fn mount(&mut self, prefix: &str, vfs: impl Vfs + 'static) -> Result<()> {
        if !is_mount_id(prefix) {
            return Err(AssetsError::InvalidPath {
                path: format!("{prefix}:"),
                reason: format!("`{prefix}` is not a plugin id"),
            });
        }
        if self.mounts.contains_key(prefix) {
            return Err(AssetsError::DuplicateMount(prefix.to_owned()));
        }
        self.mounts.insert(prefix.to_owned(), Box::new(vfs));
        Ok(())
    }

    /// Mounts plugin `id`'s `plugins/<id>/assets/` folder of `project` under `id:` (§9.4,
    /// §13), so `id:ui/icon.png` reads `plugins/<id>/assets/ui/icon.png`. A plugin without an
    /// `assets/` folder is not mounted; returns whether it was.
    pub fn mount_plugin_assets<V: Vfs + 'static>(&mut self, id: &str, project: V) -> Result<bool> {
        let dir = VfsPath::new(&format!("plugins/{id}/assets"))?;
        if project.list(&dir).is_err() {
            return Ok(false);
        }
        self.mount(id, SubVfs::new(project, dir))?;
        Ok(true)
    }

    /// Removes a mount; returns `true` if it existed. Cached assets are kept until evicted.
    pub fn unmount(&mut self, prefix: &str) -> bool {
        self.mounts.remove(prefix).is_some()
    }

    /// Mounted prefixes, sorted.
    pub fn mounts(&self) -> impl Iterator<Item = &str> {
        self.mounts.keys().map(String::as_str)
    }

    fn vfs_for(&self, path: &AssetPath) -> Result<&dyn Vfs> {
        match path.mount() {
            None => Ok(self.root.as_ref()),
            Some(prefix) => self
                .mounts
                .get(prefix)
                .map(|v| v.as_ref())
                .ok_or_else(|| AssetsError::UnknownMount(prefix.to_owned())),
        }
    }

    /// Reads raw bytes (`"sprites/hero.png"` or `"resonance:ui/icon.png"`), uncached.
    pub fn read(&self, path: &str) -> Result<Vec<u8>> {
        self.read_path(&AssetPath::parse(path)?)
    }

    /// Reads raw bytes from a parsed path, uncached.
    pub fn read_path(&self, path: &AssetPath) -> Result<Vec<u8>> {
        self.vfs_for(path)?.read(path.path())
    }

    /// Returns `true` if a file exists at `path` (invalid paths and unknown mounts are `false`).
    pub fn exists(&self, path: &str) -> bool {
        AssetPath::parse(path)
            .ok()
            .and_then(|p| Some(self.vfs_for(&p).ok()?.exists(p.path())))
            .unwrap_or(false)
    }

    /// Lists the direct children of a directory (`""` is the project root, `"resonance:"` a mount root).
    pub fn list(&self, dir: &str) -> Result<Vec<AssetEntry>> {
        let dir = AssetPath::parse(dir)?;
        let entries = self.vfs_for(&dir)?.list(dir.path())?;
        entries
            .into_iter()
            .map(|e| {
                let path = match dir.mount() {
                    Some(prefix) => AssetPath::in_mount(prefix, e.path)?,
                    None => AssetPath::project(e.path),
                };
                Ok(AssetEntry { path, kind: e.kind })
            })
            .collect()
    }

    /// Loads and decodes `path` on first request, then serves it from the cache.
    /// Each call adds a reference; pair it with [`release`](Self::release).
    pub fn load<T: Asset>(&mut self, path: &str) -> Result<Handle<T>> {
        let path = AssetPath::parse(path)?;
        self.load_path(&path)
    }

    /// [`load`](Self::load) for an already-parsed path.
    pub fn load_path<T: Asset>(&mut self, path: &AssetPath) -> Result<Handle<T>> {
        let Self {
            root,
            mounts,
            stores,
        } = self;
        let store = stores
            .entry(TypeId::of::<T>())
            .or_insert_with(|| Box::new(AssetStore::<T>::new()))
            .as_any_mut()
            .downcast_mut::<AssetStore<T>>()
            .ok_or(AssetsError::StaleHandle)?;
        store.get_or_load(path, |path| {
            let vfs: &dyn Vfs = match path.mount() {
                None => root.as_ref(),
                Some(prefix) => mounts
                    .get(prefix)
                    .map(|v| v.as_ref())
                    .ok_or_else(|| AssetsError::UnknownMount(prefix.to_owned()))?,
            };
            T::decode(vfs.read(path.path())?, path)
        })
    }

    /// The typed store for `T`, if anything of that type was ever loaded.
    pub fn store<T: Asset>(&self) -> Option<&AssetStore<T>> {
        self.stores
            .get(&TypeId::of::<T>())
            .and_then(|s| s.as_any().downcast_ref())
    }

    fn store_mut<T: Asset>(&mut self) -> Option<&mut AssetStore<T>> {
        self.stores
            .get_mut(&TypeId::of::<T>())
            .and_then(|s| s.as_any_mut().downcast_mut())
    }

    /// The asset behind `handle`, if still cached.
    pub fn get<T: Asset>(&self, handle: Handle<T>) -> Option<&T> {
        self.store::<T>()?.get(handle)
    }

    /// Like [`get`](Self::get) but a stale handle is an error.
    pub fn require<T: Asset>(&self, handle: Handle<T>) -> Result<&T> {
        self.get(handle).ok_or(AssetsError::StaleHandle)
    }

    /// The path `handle` was loaded from.
    pub fn path_of<T: Asset>(&self, handle: Handle<T>) -> Option<&AssetPath> {
        self.store::<T>()?.path_of(handle)
    }

    /// Adds a reference to a cached asset.
    pub fn acquire<T: Asset>(&mut self, handle: Handle<T>) -> Result<()> {
        self.store_mut::<T>()
            .ok_or(AssetsError::StaleHandle)?
            .acquire(handle)
    }

    /// Drops a reference; returns the remaining count.
    pub fn release<T: Asset>(&mut self, handle: Handle<T>) -> Result<u32> {
        self.store_mut::<T>()
            .ok_or(AssetsError::StaleHandle)?
            .release(handle)
    }

    /// Evicts every unreferenced asset of every type; returns how many were dropped.
    pub fn evict_unused(&mut self) -> usize {
        self.stores.values_mut().map(|s| s.evict_unused()).sum()
    }

    /// Total number of cached assets across all types.
    pub fn cached_count(&self) -> usize {
        self.stores.values().map(|s| s.len()).sum()
    }

    /// Normalises `path` without touching storage (useful for keys and diagnostics).
    pub fn normalize(path: &str) -> Result<AssetPath> {
        AssetPath::parse(path)
    }

    /// The project root as a listing target.
    pub fn root_dir() -> VfsPath {
        VfsPath::root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::{RawBytes, Text};
    use crate::vfs::{MemoryVfs, VfsEntry};
    use core::cell::Cell;
    use std::rc::Rc;

    /// Counts reads to prove cache hits.
    struct Counting {
        inner: MemoryVfs,
        reads: Rc<Cell<u32>>,
    }

    impl Vfs for Counting {
        fn read(&self, path: &VfsPath) -> Result<Vec<u8>> {
            self.reads.set(self.reads.get() + 1);
            self.inner.read(path)
        }
        fn exists(&self, path: &VfsPath) -> bool {
            self.inner.exists(path)
        }
        fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>> {
            self.inner.list(dir)
        }
    }

    fn project() -> MemoryVfs {
        MemoryVfs::new()
            .with("sprites/hero.png", b"PNG".to_vec())
            .and_then(|v| v.with("data/types.ron", b"(fire)".to_vec()))
            .expect("valid")
    }

    fn plugin() -> MemoryVfs {
        MemoryVfs::new()
            .with("ui/icon.png", b"ICON".to_vec())
            .expect("valid")
    }

    fn assets() -> Assets {
        let mut assets = Assets::new(project());
        assets.mount("resonance", plugin()).expect("mount");
        assets
    }

    #[test]
    fn reads_route_through_mounts() {
        let assets = assets();
        assert_eq!(assets.read("sprites/hero.png").expect("root"), b"PNG");
        assert_eq!(
            assets.read("resonance:ui/icon.png").expect("mount"),
            b"ICON"
        );
        assert!(assets.exists("resonance:ui/icon.png"));
        assert!(
            !assets.exists("ui/icon.png"),
            "plugin files are not in the root"
        );
        assert!(!assets.exists("../x"));
        assert!(!assets.exists("nope:x"));
        assert!(matches!(
            assets.read("nope:ui/icon.png"),
            Err(AssetsError::UnknownMount(m)) if m == "nope"
        ));
        assert_eq!(assets.mounts().collect::<Vec<_>>(), ["resonance"]);
    }

    #[test]
    fn traversal_is_rejected_at_the_facade() {
        let assets = assets();
        for raw in [
            "../pkrs.toml",
            "sprites/../../x",
            "resonance:../sprites/hero.png",
        ] {
            assert!(
                matches!(assets.read(raw), Err(AssetsError::PathEscapesRoot(_))),
                "{raw}"
            );
        }
    }

    #[test]
    fn mount_rules() {
        let mut assets = assets();
        assert!(matches!(
            assets.mount("resonance", MemoryVfs::new()),
            Err(AssetsError::DuplicateMount(_))
        ));
        assert!(assets.mount("Bad", MemoryVfs::new()).is_err());
        assert!(assets.unmount("resonance"));
        assert!(!assets.unmount("resonance"));
        assert!(!assets.exists("resonance:ui/icon.png"));
    }

    #[test]
    fn list_keeps_mount_prefix() {
        let assets = assets();
        let root: Vec<String> = assets
            .list("")
            .expect("root")
            .into_iter()
            .map(|e| e.path.to_string())
            .collect();
        assert_eq!(root, ["data", "sprites"]);
        let plugin = assets.list("resonance:ui").expect("mount dir");
        assert_eq!(
            plugin,
            [AssetEntry {
                path: AssetPath::parse("resonance:ui/icon.png").expect("valid"),
                kind: EntryKind::File
            }]
        );
        assert_eq!(assets.list("resonance:").expect("mount root").len(), 1);
    }

    #[test]
    fn load_caches_per_path_and_type() {
        let reads = Rc::new(Cell::new(0));
        let mut assets = Assets::new(Counting {
            inner: project(),
            reads: Rc::clone(&reads),
        });
        let a: Handle<Text> = assets.load("data/types.ron").expect("load");
        let b: Handle<Text> = assets.load("data/./types.ron").expect("hit");
        assert_eq!(a, b);
        assert_eq!(reads.get(), 1, "second load is a cache hit");
        assert_eq!(assets.require(a).expect("live").0, "(fire)");
        assert_eq!(
            assets.path_of(a).map(ToString::to_string).as_deref(),
            Some("data/types.ron")
        );

        // Same path, different type: separate cache entry.
        let raw: Handle<RawBytes> = assets.load("data/types.ron").expect("load");
        assert_eq!(reads.get(), 2);
        assert_eq!(assets.get(raw).map(|r| r.0.len()), Some(6));
        assert_eq!(assets.cached_count(), 2);
        assert_eq!(assets.store::<Text>().and_then(|s| s.refs(a)), Some(2));
    }

    #[test]
    fn release_and_evict_across_types() {
        let mut assets = assets();
        let text: Handle<Text> = assets.load("data/types.ron").expect("load");
        let icon: Handle<RawBytes> = assets.load("resonance:ui/icon.png").expect("load");
        assets.acquire(icon).expect("live");
        assert_eq!(assets.release(text).expect("live"), 0);
        assert_eq!(assets.release(icon).expect("live"), 1);
        assert_eq!(assets.evict_unused(), 1);
        assert!(assets.get(text).is_none());
        assert!(matches!(
            assets.require(text),
            Err(AssetsError::StaleHandle)
        ));
        assert!(assets.get(icon).is_some());

        let never: Handle<Text> = assets.load("data/types.ron").expect("reload");
        assert_ne!(never, text);
    }

    #[test]
    fn failed_loads_are_not_cached() {
        let mut assets = assets();
        assert!(matches!(
            assets.load::<Text>("missing.ron"),
            Err(AssetsError::NotFound(_))
        ));
        assert!(matches!(
            assets.load::<Text>("x:missing.ron"),
            Err(AssetsError::UnknownMount(_))
        ));
        assert_eq!(assets.cached_count(), 0);
        assert!(matches!(
            assets.release(Handle::<RawBytes>::default_for_tests()),
            Err(AssetsError::StaleHandle)
        ));
    }

    #[test]
    fn debug_lists_mounts() {
        let rendered = format!("{:?}", assets());
        assert!(rendered.contains("resonance"), "{rendered}");
        assert_eq!(
            Assets::normalize("a/./b").expect("valid").to_string(),
            "a/b"
        );
        assert!(Assets::root_dir().is_root());
    }
}
