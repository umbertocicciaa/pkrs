//! Integration (§12 acceptance): a crash at any step of a save write leaves the previous save
//! loadable. The crash is injected through the [`SaveFs`] port on top of the real file system.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};

use pkrs_save::{SaveData, SaveError, SaveFs, SaveLoader, SaveSlot, SaveStore, StdFs};

/// Where the simulated crash happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Crash {
    /// Half of the temporary file is written, then the process dies (no fsync).
    WhileWritingTemp,
    /// The temporary file is complete, the backup copy is torn.
    WhileWritingBackup,
    /// Everything is written, the rename never happens.
    BeforeRename,
}

/// [`StdFs`] that "crashes" (fails every later operation) at one point.
struct CrashFs {
    crash: Crash,
    dead: Cell<bool>,
}

impl CrashFs {
    fn new(crash: Crash) -> Self {
        Self {
            crash,
            dead: Cell::new(false),
        }
    }

    fn die(&self) -> io::Error {
        self.dead.set(true);
        io::Error::other("simulated crash")
    }

    fn check(&self) -> io::Result<()> {
        if self.dead.get() {
            Err(io::Error::other("process is dead"))
        } else {
            Ok(())
        }
    }
}

fn is_temp(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".tmp")
}

fn is_backup(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".bak")
}

impl SaveFs for CrashFs {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.check()?;
        StdFs.read(path)
    }

    fn write_synced(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.check()?;
        let torn = match self.crash {
            Crash::WhileWritingTemp => is_temp(path),
            Crash::WhileWritingBackup => is_backup(path),
            Crash::BeforeRename => false,
        };
        if torn {
            std::fs::write(path, &bytes[..bytes.len() / 2])?;
            return Err(self.die());
        }
        StdFs.write_synced(path, bytes)
    }

    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.check()?;
        if self.crash == Crash::BeforeRename {
            return Err(self.die());
        }
        StdFs.rename(from, to)
    }

    fn remove(&self, path: &Path) -> io::Result<()> {
        self.check()?;
        StdFs.remove(path)
    }

    fn create_dir_all(&self, dir: &Path) -> io::Result<()> {
        self.check()?;
        StdFs.create_dir_all(dir)
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    std::env::temp_dir().join(format!("pkrs-save-{tag}-{}-{nanos}", std::process::id()))
}

fn save(money: i64) -> SaveData {
    let mut data = SaveData::default();
    data.world.money = money;
    data.world.set_flag("progress", money > 1);
    data
}

fn loaded_money(dir: &Path, slot: SaveSlot) -> (i64, bool) {
    let loaded = SaveStore::new(dir)
        .load(slot, &mut SaveLoader::new())
        .unwrap();
    (loaded.data.world.money, loaded.from_backup)
}

#[test]
fn a_crash_at_any_step_keeps_the_previous_save() {
    for crash in [
        Crash::WhileWritingTemp,
        Crash::WhileWritingBackup,
        Crash::BeforeRename,
    ] {
        for slot in [SaveSlot::Slot(1), SaveSlot::Autosave] {
            let dir = temp_dir("crash");
            let good = SaveStore::new(&dir);
            good.write(slot, &save(1)).unwrap();

            let crashing = SaveStore::with_fs(&dir, Box::new(CrashFs::new(crash)));
            let err = crashing.write(slot, &save(2)).unwrap_err();
            assert!(matches!(err, SaveError::Io { .. }), "{crash:?}: {err}");

            // After "restarting", the previous save is still the one in the slot.
            assert_eq!(loaded_money(&dir, slot), (1, false), "{crash:?} {slot}");

            // The next successful write replaces it, and the backup holds the old one.
            good.write(slot, &save(3)).unwrap();
            assert_eq!(loaded_money(&dir, slot), (3, false), "{crash:?} {slot}");
            let bak = std::fs::read_to_string(good.backup_path(slot)).unwrap();
            assert!(bak.contains("\"money\": 1"), "{crash:?}: {bak}");
            assert!(!good.temp_path(slot).exists());
            std::fs::remove_dir_all(&dir).unwrap();
        }
    }
}

#[test]
fn a_first_save_that_crashes_leaves_the_slot_empty() {
    let dir = temp_dir("first");
    let crashing = SaveStore::with_fs(&dir, Box::new(CrashFs::new(Crash::BeforeRename)));
    assert!(crashing.write(SaveSlot::Slot(1), &save(1)).is_err());
    let err = SaveStore::new(&dir)
        .load(SaveSlot::Slot(1), &mut SaveLoader::new())
        .unwrap_err();
    assert!(matches!(err, SaveError::NotFound(_)), "{err}");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_torn_main_file_loads_the_backup() {
    let dir = temp_dir("torn");
    let store = SaveStore::new(&dir);
    let slot = SaveSlot::Slot(2);
    store.write(slot, &save(1)).unwrap();
    store.write(slot, &save(2)).unwrap();
    // E.g. a disk error or a tool that wrote the file in place.
    let text = std::fs::read_to_string(store.path(slot)).unwrap();
    std::fs::write(store.path(slot), &text[..text.len() / 3]).unwrap();
    assert_eq!(loaded_money(&dir, slot), (1, true));
    // A missing main file loads the backup too.
    std::fs::remove_file(store.path(slot)).unwrap();
    assert_eq!(loaded_money(&dir, slot), (1, true));
    std::fs::remove_dir_all(&dir).unwrap();
}
