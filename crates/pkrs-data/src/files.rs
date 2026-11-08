//! [`DataFiles`]: the read-only file port the content loader reads `data/` through.
//!
//! `pkrs-data` does not depend on `pkrs-assets` (§4.3); the runtime and the CLI implement
//! this port over the project's VFS. [`MemoryDataFiles`] serves tests and tools.

use std::collections::{BTreeMap, BTreeSet};

/// Read-only access to project files, with forward-slash project-relative paths.
pub trait DataFiles {
    /// Names (not paths) of the files directly in `dir`; empty when `dir` does not exist.
    fn list_files(&self, dir: &str) -> Vec<String>;

    /// Names of the folders directly in `dir`; empty when `dir` does not exist.
    fn list_dirs(&self, dir: &str) -> Vec<String>;

    /// `true` if the file `path` exists.
    fn exists(&self, path: &str) -> bool;

    /// The UTF-8 text of `path`, or why it cannot be read.
    fn read_text(&self, path: &str) -> Result<String, String>;
}

/// In-memory [`DataFiles`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryDataFiles {
    files: BTreeMap<String, String>,
}

impl MemoryDataFiles {
    /// No files.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds (or replaces) `path` with `text`.
    pub fn insert(&mut self, path: impl Into<String>, text: impl Into<String>) {
        self.files.insert(path.into(), text.into());
    }

    /// Builder form of [`insert`](Self::insert).
    #[must_use]
    pub fn with(mut self, path: impl Into<String>, text: impl Into<String>) -> Self {
        self.insert(path, text);
        self
    }

    /// Entries directly below `dir`: `(name, is_file)`.
    fn children(&self, dir: &str) -> BTreeSet<(String, bool)> {
        let prefix = format!("{}/", dir.trim_end_matches('/'));
        self.files
            .keys()
            .filter_map(|path| path.strip_prefix(&prefix))
            .map(|rest| match rest.split_once('/') {
                Some((folder, _)) => (folder.to_owned(), false),
                None => (rest.to_owned(), true),
            })
            .collect()
    }
}

impl DataFiles for MemoryDataFiles {
    fn list_files(&self, dir: &str) -> Vec<String> {
        self.children(dir)
            .into_iter()
            .filter_map(|(name, file)| file.then_some(name))
            .collect()
    }

    fn list_dirs(&self, dir: &str) -> Vec<String> {
        self.children(dir)
            .into_iter()
            .filter_map(|(name, file)| (!file).then_some(name))
            .collect()
    }

    fn exists(&self, path: &str) -> bool {
        self.files.contains_key(path)
    }

    fn read_text(&self, path: &str) -> Result<String, String> {
        self.files
            .get(path)
            .cloned()
            .ok_or_else(|| "file not found".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_files_and_folders() {
        let files = MemoryDataFiles::new()
            .with("data/trainers/b.ron", "")
            .with("data/trainers/a.ron", "")
            .with("data/trainers/profiles/p.ron", "")
            .with("data/types.ron", "");
        assert_eq!(files.list_files("data/trainers"), ["a.ron", "b.ron"]);
        assert_eq!(files.list_dirs("data/trainers"), ["profiles"]);
        assert_eq!(files.list_dirs("data"), ["trainers"]);
        assert_eq!(files.list_files("data"), ["types.ron"]);
        assert!(files.list_files("nope").is_empty());
        assert!(files.exists("data/types.ron"));
        assert!(files.read_text("data/x.ron").is_err());
    }
}
