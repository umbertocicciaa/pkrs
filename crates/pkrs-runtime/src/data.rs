//! Project content (§8.1) read through the assets VFS: the [`DataFiles`] port of
//! `pkrs-data` implemented over a [`Vfs`], and the loader the runtime uses.

use pkrs_assets::{EntryKind, Vfs, VfsPath};
use pkrs_data::{
    ContentDb, DATA_ROOT, DataErrors, DataFiles, GameData, GameDataBuilder, LoadOptions,
};

/// [`DataFiles`] over a mounted project ([`Vfs`]).
pub struct VfsDataFiles<'a>(pub &'a dyn Vfs);

impl VfsDataFiles<'_> {
    fn names(&self, dir: &str, kind: EntryKind) -> Vec<String> {
        let Ok(dir) = VfsPath::new(dir) else {
            return Vec::new();
        };
        let mut names: Vec<String> = self
            .0
            .list(&dir)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| e.kind == kind)
            .filter_map(|e| e.path.file_name().map(str::to_owned))
            .collect();
        names.sort();
        names
    }
}

impl DataFiles for VfsDataFiles<'_> {
    fn list_files(&self, dir: &str) -> Vec<String> {
        self.names(dir, EntryKind::File)
    }

    fn list_dirs(&self, dir: &str) -> Vec<String> {
        self.names(dir, EntryKind::Dir)
    }

    fn exists(&self, path: &str) -> bool {
        VfsPath::new(path).is_ok_and(|p| self.0.exists(&p))
    }

    fn read_text(&self, path: &str) -> Result<String, String> {
        let path = VfsPath::new(path).map_err(|e| e.to_string())?;
        let bytes = self.0.read(&path).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|_| "not valid UTF-8".to_owned())
    }
}

/// Loads the project's `data/` folder from `vfs` with `options` and, when given, merges the
/// plugins' content database `content` (§9.4) before freezing it.
pub fn load_game_data(
    vfs: &dyn Vfs,
    options: LoadOptions,
    content: Option<&ContentDb>,
) -> Result<GameData, DataErrors> {
    let mut builder = GameDataBuilder::new(options);
    builder.load_dir(&VfsDataFiles(vfs), DATA_ROOT);
    if let Some(db) = content {
        builder.merge_content(db);
    }
    builder.build()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use pkrs_assets::MemoryVfs;

    #[test]
    fn reads_through_the_vfs() {
        let mut vfs = MemoryVfs::new();
        vfs.insert("data/items/potion.ron", br#"(id: "potion")"#.to_vec())
            .unwrap();
        vfs.insert("data/text/en/a.ron", br#"{ "a": "A" }"#.to_vec())
            .unwrap();
        let files = VfsDataFiles(&vfs);
        assert_eq!(files.list_files("data/items"), ["potion.ron"]);
        assert_eq!(files.list_dirs("data/text"), ["en"]);
        assert!(files.list_files("data/nope").is_empty());
        assert!(files.exists("data/items/potion.ron"));
        let data = load_game_data(&vfs, LoadOptions::default(), None).unwrap();
        assert!(data.items().contains("potion"));
        assert_eq!(data.text().get("en", "a"), Some("A"));
    }
}
