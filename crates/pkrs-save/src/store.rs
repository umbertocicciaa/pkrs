//! [`SaveStore`]: save slots on disk with atomic writes and a backup (§12).
//!
//! Writing slot `n` goes through `slotN.sav.tmp` → `fsync` → rename over `slotN.sav`, after
//! copying the previous good `slotN.sav` to `slotN.sav.bak`. A crash at any point leaves either
//! the old or the new save in `slotN.sav` (and the old one in `.bak` once the copy finished).
//! Loading falls back to the `.bak` when `slotN.sav` is missing or cannot be decoded.

use core::fmt;
use std::path::{Path, PathBuf};

use crate::codec::RonCodec;
use crate::error::{Result, SaveError};
use crate::fs::{SaveFs, StdFs};
use crate::migrate::{SaveLoader, format_version};
use crate::model::{SaveData, SaveHeader};

/// Numbered slots a store has unless configured otherwise (§12).
pub const DEFAULT_SLOTS: u8 = 3;

/// File extension of save files.
pub const SAVE_EXTENSION: &str = "sav";

/// One save slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SaveSlot {
    /// Numbered slot (`1..=slots`), stored as `slotN.sav`.
    Slot(u8),
    /// The autosave slot, stored as `autosave.sav`.
    Autosave,
}

impl SaveSlot {
    /// File name of the slot's save.
    #[must_use]
    pub fn file_name(self) -> String {
        match self {
            Self::Slot(n) => format!("slot{n}.{SAVE_EXTENSION}"),
            Self::Autosave => format!("autosave.{SAVE_EXTENSION}"),
        }
    }
}

impl fmt::Display for SaveSlot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Slot(n) => write!(f, "slot {n}"),
            Self::Autosave => f.write_str("autosave"),
        }
    }
}

/// A loaded save and where it came from.
#[derive(Debug)]
pub struct Loaded {
    /// The save.
    pub data: SaveData,
    /// It was read from the `.bak` because the main file failed.
    pub from_backup: bool,
    /// Why the main file failed, when the backup was used.
    pub main_error: Option<SaveError>,
}

/// What the slot screen shows for one slot (§12: headers only).
#[derive(Debug)]
pub enum SlotInfo {
    /// No save.
    Empty,
    /// A save; `from_backup` when only its `.bak` is readable.
    Saved {
        /// The save's header (migrated to the current format).
        header: SaveHeader,
        /// Read from the backup.
        from_backup: bool,
    },
    /// Neither the save nor its backup can be read.
    Corrupt(String),
}

/// Save slots in one directory.
pub struct SaveStore {
    dir: PathBuf,
    fs: Box<dyn SaveFs>,
    slots: u8,
    autosave: bool,
}

impl fmt::Debug for SaveStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SaveStore")
            .field("dir", &self.dir)
            .field("slots", &self.slots)
            .field("autosave", &self.autosave)
            .finish_non_exhaustive()
    }
}

impl SaveStore {
    /// [`DEFAULT_SLOTS`] slots plus autosave in `dir`, on the real file system. The directory
    /// is created on the first write.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self::with_fs(dir, Box::new(StdFs))
    }

    /// Like [`new`](Self::new), through `fs`.
    pub fn with_fs(dir: impl Into<PathBuf>, fs: Box<dyn SaveFs>) -> Self {
        Self {
            dir: dir.into(),
            fs,
            slots: DEFAULT_SLOTS,
            autosave: true,
        }
    }

    /// Sets the number of numbered slots.
    #[must_use]
    pub fn with_slots(mut self, slots: u8) -> Self {
        self.slots = slots;
        self
    }

    /// Enables or disables the autosave slot.
    #[must_use]
    pub fn with_autosave(mut self, autosave: bool) -> Self {
        self.autosave = autosave;
        self
    }

    /// The save directory.
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Every slot, numbered slots first, then autosave if enabled.
    #[must_use]
    pub fn slots(&self) -> Vec<SaveSlot> {
        let mut slots: Vec<SaveSlot> = (1..=self.slots).map(SaveSlot::Slot).collect();
        if self.autosave {
            slots.push(SaveSlot::Autosave);
        }
        slots
    }

    fn check(&self, slot: SaveSlot) -> Result<()> {
        let known = match slot {
            SaveSlot::Slot(n) => (1..=self.slots).contains(&n),
            SaveSlot::Autosave => self.autosave,
        };
        if known {
            Ok(())
        } else {
            Err(SaveError::UnknownSlot(slot.to_string()))
        }
    }

    /// Path of `slot`'s save file.
    #[must_use]
    pub fn path(&self, slot: SaveSlot) -> PathBuf {
        self.dir.join(slot.file_name())
    }

    /// Path of `slot`'s backup (`slotN.sav.bak`).
    #[must_use]
    pub fn backup_path(&self, slot: SaveSlot) -> PathBuf {
        self.dir.join(format!("{}.bak", slot.file_name()))
    }

    /// Path of `slot`'s temporary file during a write (`slotN.sav.tmp`).
    #[must_use]
    pub fn temp_path(&self, slot: SaveSlot) -> PathBuf {
        self.dir.join(format!("{}.tmp", slot.file_name()))
    }

    /// Writes `save` into `slot` atomically, keeping the previous good save as `.bak`.
    pub fn write(&self, slot: SaveSlot, save: &SaveData) -> Result<()> {
        self.check(slot)?;
        let text = save.to_ron()?;
        self.write_text(slot, &text)
    }

    fn write_text(&self, slot: SaveSlot, text: &str) -> Result<()> {
        let fs = self.fs.as_ref();
        fs.create_dir_all(&self.dir)
            .map_err(|e| SaveError::io(&self.dir, e))?;
        let main = self.path(slot);
        let tmp = self.temp_path(slot);
        let bak = self.backup_path(slot);

        // 1. The new save goes to `.tmp`, flushed to disk.
        if let Err(e) = fs.write_synced(&tmp, text.as_bytes()) {
            let _ = fs.remove(&tmp);
            return Err(SaveError::io(&tmp, e));
        }
        // 2. The previous save becomes the backup, if it is a save at all: a corrupt main file
        //    must not replace a good backup.
        match fs.read(&main) {
            Ok(old) if looks_like_save(&old) => {
                fs.write_synced(&bak, &old)
                    .map_err(|e| SaveError::io(&bak, e))?;
            }
            Ok(_) => {
                tracing::warn!(target: "save", path = %main.display(), "previous save is corrupt; keeping the old backup");
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(SaveError::io(&main, e)),
        }
        // 3. Atomic replace.
        fs.rename(&tmp, &main)
            .map_err(|e| SaveError::io(&main, e))?;
        if let Err(e) = fs.sync_dir(&self.dir) {
            tracing::debug!(target: "save", dir = %self.dir.display(), %e, "directory sync failed");
        }
        tracing::info!(target: "save", %slot, path = %main.display(), "saved");
        Ok(())
    }

    fn load_file(&self, path: &Path, loader: &mut SaveLoader<'_>) -> Result<SaveData> {
        let text = self.read_text(path)?;
        loader.decode(&text, &file_label(path))
    }

    fn read_text(&self, path: &Path) -> Result<String> {
        let bytes = self.fs.read(path).map_err(|e| SaveError::io(path, e))?;
        String::from_utf8(bytes)
            .map_err(|_| SaveError::Codec(format!("{}: not UTF-8", file_label(path))))
    }

    /// Loads `slot`, falling back to its `.bak` when the main file is missing or corrupt.
    pub fn load(&self, slot: SaveSlot, loader: &mut SaveLoader<'_>) -> Result<Loaded> {
        self.check(slot)?;
        let main = self.path(slot);
        let main_err = match self.load_file(&main, loader) {
            Ok(data) => {
                return Ok(Loaded {
                    data,
                    from_backup: false,
                    main_error: None,
                });
            }
            Err(e) => e,
        };
        let bak = self.backup_path(slot);
        match self.load_file(&bak, loader) {
            Ok(data) => {
                tracing::warn!(target: "save", %slot, error = %main_err, "save unreadable; loaded the backup");
                Ok(Loaded {
                    data,
                    from_backup: true,
                    main_error: Some(main_err),
                })
            }
            Err(bak_err) if bak_err.is_not_found() => {
                if main_err.is_not_found() {
                    Err(SaveError::NotFound(main))
                } else {
                    Err(main_err)
                }
            }
            Err(bak_err) => Err(SaveError::Unrecoverable {
                path: main,
                main: Box::new(main_err),
                backup: Box::new(bak_err),
            }),
        }
    }

    /// The header of `slot` for the slot screen (falls back to the backup like
    /// [`load`](Self::load)).
    #[must_use]
    pub fn info(&self, slot: SaveSlot, loader: &SaveLoader<'_>) -> SlotInfo {
        let header = |path: &Path| -> Result<SaveHeader> {
            let text = self.read_text(path)?;
            loader.decode_header(&text, &file_label(path))
        };
        match header(&self.path(slot)) {
            Ok(header) => SlotInfo::Saved {
                header,
                from_backup: false,
            },
            Err(main_err) => match header(&self.backup_path(slot)) {
                Ok(header) => SlotInfo::Saved {
                    header,
                    from_backup: true,
                },
                Err(bak_err) if bak_err.is_not_found() && main_err.is_not_found() => {
                    SlotInfo::Empty
                }
                Err(bak_err) if bak_err.is_not_found() => SlotInfo::Corrupt(main_err.to_string()),
                Err(bak_err) => SlotInfo::Corrupt(format!("{main_err}; backup: {bak_err}")),
            },
        }
    }

    /// [`info`](Self::info) for every slot.
    #[must_use]
    pub fn list(&self, loader: &SaveLoader<'_>) -> Vec<(SaveSlot, SlotInfo)> {
        self.slots()
            .into_iter()
            .map(|slot| (slot, self.info(slot, loader)))
            .collect()
    }

    /// Deletes `slot`'s save, backup and any leftover temporary file.
    pub fn delete(&self, slot: SaveSlot) -> Result<()> {
        self.check(slot)?;
        for path in [
            self.path(slot),
            self.backup_path(slot),
            self.temp_path(slot),
        ] {
            match self.fs.remove(&path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(SaveError::io(path, e)),
            }
        }
        Ok(())
    }
}

/// `true` when `bytes` parse as a save tree with a format version (cheap check before a file
/// is kept as the backup).
fn looks_like_save(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| RonCodec::decode(text, "").ok())
        .is_some_and(|value| format_version(&value).is_ok())
}

fn file_label(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::io;

    use super::*;

    /// In-memory file system.
    #[derive(Default)]
    struct MemoryFs {
        files: RefCell<BTreeMap<PathBuf, Vec<u8>>>,
    }

    fn not_found() -> io::Error {
        io::Error::from(io::ErrorKind::NotFound)
    }

    impl SaveFs for MemoryFs {
        fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
            self.files.borrow().get(path).cloned().ok_or_else(not_found)
        }
        fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
            self.files
                .borrow_mut()
                .insert(path.to_owned(), bytes.to_vec());
            Ok(())
        }
        fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
            let mut files = self.files.borrow_mut();
            let bytes = files.remove(from).ok_or_else(not_found)?;
            files.insert(to.to_owned(), bytes);
            Ok(())
        }
        fn remove(&self, path: &Path) -> io::Result<()> {
            self.files
                .borrow_mut()
                .remove(path)
                .map(|_| ())
                .ok_or_else(not_found)
        }
        fn create_dir_all(&self, _dir: &Path) -> io::Result<()> {
            Ok(())
        }
    }

    fn save(money: i64) -> SaveData {
        let mut data = SaveData::default();
        data.world.money = money;
        data
    }

    fn store() -> SaveStore {
        SaveStore::with_fs("saves", Box::new(MemoryFs::default()))
    }

    #[test]
    fn slots_and_file_names() {
        let store = store().with_slots(2);
        assert_eq!(
            store.slots(),
            [SaveSlot::Slot(1), SaveSlot::Slot(2), SaveSlot::Autosave]
        );
        assert_eq!(
            store.path(SaveSlot::Slot(1)),
            Path::new("saves").join("slot1.sav")
        );
        assert_eq!(
            store.backup_path(SaveSlot::Autosave),
            Path::new("saves").join("autosave.sav.bak")
        );
        assert!(matches!(
            store.write(SaveSlot::Slot(3), &save(1)),
            Err(SaveError::UnknownSlot(ref s)) if s == "slot 3"
        ));
        let store = store.with_autosave(false);
        assert_eq!(store.slots().len(), 2);
        assert!(store.write(SaveSlot::Autosave, &save(1)).is_err());
    }

    #[test]
    fn write_then_load_and_keep_a_backup() {
        let store = store();
        let slot = SaveSlot::Slot(1);
        assert!(matches!(
            store.load(slot, &mut SaveLoader::new()),
            Err(SaveError::NotFound(_))
        ));
        assert!(matches!(
            store.info(slot, &SaveLoader::new()),
            SlotInfo::Empty
        ));
        store.write(slot, &save(1)).unwrap();
        store.write(slot, &save(2)).unwrap();
        let loaded = store.load(slot, &mut SaveLoader::new()).unwrap();
        assert_eq!(loaded.data.world.money, 2);
        assert!(!loaded.from_backup);
        // The backup holds the previous save; no temporary file is left behind.
        let fs_files = store.fs.read(&store.backup_path(slot)).unwrap();
        assert!(
            String::from_utf8(fs_files)
                .unwrap()
                .contains("\"money\": 1")
        );
        assert!(store.fs.read(&store.temp_path(slot)).is_err());
        assert!(matches!(
            store.info(slot, &SaveLoader::new()),
            SlotInfo::Saved {
                from_backup: false,
                ..
            }
        ));
    }

    #[test]
    fn corrupt_main_file_falls_back_to_the_backup_and_is_not_backed_up() {
        let store = store();
        let slot = SaveSlot::Slot(2);
        store.write(slot, &save(1)).unwrap();
        store.write(slot, &save(2)).unwrap();
        store
            .fs
            .write_synced(&store.path(slot), b"{ \"header\": ")
            .unwrap();
        let loaded = store.load(slot, &mut SaveLoader::new()).unwrap();
        assert!(loaded.from_backup);
        assert_eq!(loaded.data.world.money, 1);
        assert!(matches!(loaded.main_error, Some(SaveError::Codec(_))));
        assert!(matches!(
            store.info(slot, &SaveLoader::new()),
            SlotInfo::Saved {
                from_backup: true,
                ..
            }
        ));
        // Saving over a corrupt file keeps the good backup.
        store.write(slot, &save(3)).unwrap();
        let bak = store.fs.read(&store.backup_path(slot)).unwrap();
        assert!(String::from_utf8(bak).unwrap().contains("\"money\": 1"));
    }

    #[test]
    fn both_files_corrupt_is_unrecoverable() {
        let store = store();
        let slot = SaveSlot::Autosave;
        store.fs.write_synced(&store.path(slot), b"nope").unwrap();
        assert!(matches!(
            store.load(slot, &mut SaveLoader::new()),
            Err(SaveError::Codec(_))
        ));
        store
            .fs
            .write_synced(&store.backup_path(slot), b"nope")
            .unwrap();
        assert!(matches!(
            store.load(slot, &mut SaveLoader::new()),
            Err(SaveError::Unrecoverable { .. })
        ));
        assert!(matches!(
            store.info(slot, &SaveLoader::new()),
            SlotInfo::Corrupt(_)
        ));
        store.delete(slot).unwrap();
        assert!(matches!(
            store.info(slot, &SaveLoader::new()),
            SlotInfo::Empty
        ));
    }
}
