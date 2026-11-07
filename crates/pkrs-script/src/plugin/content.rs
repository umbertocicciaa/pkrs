//! Content merging (§8.1, §9.4 step 4): the game's `data/` folder, then each plugin's
//! `plugins/<id>/data/` folder in load order, into one [`ContentDb`].
//!
//! Every `data/<kind>/*.ron` file (`moves`, `abilities`, `items`, `statuses`, `terrains`,
//! `effects`) holds one record or a list. Files are read in name order. Duplicate ids are
//! errors unless the later record says `override: true`, in which case the override is
//! logged (target `data`). Problems are collected, not fail-fast, so every one is reported.

use core::fmt;

use pkrs_data::{ContentDb, ContentKind, ContentOverride, ContentSource, DataError, parse_content};
use pkrs_world::Terrain;

use crate::host::ScriptFiles;

/// The game's content folder.
pub const DATA_DIR: &str = "data";

/// The content folder of plugin `id` (`plugins/<id>/data`).
#[must_use]
pub fn plugin_data_dir(id: &str) -> String {
    format!("{}/{id}/{DATA_DIR}", super::PLUGINS_DIR)
}

/// One content problem, with the file (or Lua call) it came from.
#[derive(Debug)]
pub struct ContentError {
    /// Project-relative file, or the registering function for Lua registrations.
    pub file: String,
    /// What is wrong.
    pub source: DataError,
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file, self.source)
    }
}

impl std::error::Error for ContentError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// What [`merge_content`] produced.
#[derive(Debug, Default)]
pub struct ContentMerge {
    /// The merged database (records that failed are left out).
    pub db: ContentDb,
    /// Files read.
    pub files: usize,
    /// Every problem found.
    pub errors: Vec<ContentError>,
}

/// Rejects built-in terrain names (`grass`, `water`, `ledge_s`, …): only custom terrains can
/// be defined by data or plugins (§6.3).
pub fn check_custom_terrain(name: &str) -> Result<(), String> {
    match Terrain::parse(name) {
        Ok(Terrain::Custom(_)) => Ok(()),
        Ok(_) => Err(format!(
            "`{name}` is a built-in terrain and cannot be redefined"
        )),
        Err(e) => Err(e),
    }
}

/// Merges `data/` and then `plugins/<id>/data/` for every id of `plugins` (load order).
pub fn merge_content(files: &dyn ScriptFiles, plugins: &[&str]) -> ContentMerge {
    let mut merge = ContentMerge::default();
    merge_folder(files, DATA_DIR, &ContentSource::Game, &mut merge);
    for id in plugins {
        let source = ContentSource::Plugin((*id).to_owned());
        merge_folder(files, &plugin_data_dir(id), &source, &mut merge);
    }
    merge
}

fn merge_folder(
    files: &dyn ScriptFiles,
    root: &str,
    source: &ContentSource,
    out: &mut ContentMerge,
) {
    for kind in ContentKind::ALL {
        let dir = format!("{root}/{}", kind.folder());
        for name in files.list_files(&dir) {
            if !name.ends_with(".ron") {
                continue;
            }
            let path = format!("{dir}/{name}");
            out.files += 1;
            let text = match files.read_script(&path) {
                Ok(text) => text,
                Err(e) => {
                    out.errors.push(ContentError {
                        file: path,
                        source: DataError::Parse {
                            file: name,
                            message: e.to_string(),
                        },
                    });
                    continue;
                }
            };
            let records = match parse_content(*kind, &text, &path) {
                Ok(records) => records,
                Err(source) => {
                    out.errors.push(ContentError { file: path, source });
                    continue;
                }
            };
            for record in records {
                if *kind == ContentKind::Terrain {
                    if let Err(message) = check_custom_terrain(record.def.id()) {
                        out.errors.push(ContentError {
                            file: path.clone(),
                            source: DataError::InvalidField {
                                field: "id".into(),
                                context: format!("terrain `{}`", record.def.id()),
                                message,
                            },
                        });
                        continue;
                    }
                }
                match out.db.insert(record, source.clone()) {
                    Ok(Some(ov)) => log_override(&ov, &path),
                    Ok(None) => {}
                    Err(source) => out.errors.push(ContentError {
                        file: path.clone(),
                        source,
                    }),
                }
            }
        }
    }
}

/// Logs an applied override (§9.4: "logged").
pub fn log_override(ov: &ContentOverride, origin: &str) {
    tracing::info!(
        target: "data",
        kind = ov.kind.name(),
        id = %ov.id,
        previous = %ov.previous,
        by = %ov.by,
        origin,
        "content override: {ov}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::MemoryScripts;

    const MOVE: &str = "(id: \"ember\", type: \"fire\", category: special, power: 40, \
                        accuracy: 100, priority: 0, effects: [(effect: \"damage\")])";

    #[test]
    fn game_then_plugins_in_load_order() {
        let files = MemoryScripts::new()
            .with("data/moves/ember.ron", MOVE)
            .with("data/moves/notes.txt", "ignored")
            .with(
                "plugins/hot/data/moves/ember.ron",
                MOVE.replace("power: 40", "power: 90, override: true"),
            )
            .with(
                "plugins/hot/data/terrains/lava.ron",
                "[(id: \"lava\", passable: false), (id: \"hot:ash\")]",
            )
            .with("plugins/cold/data/items/ice.ron", "(id: \"ice\", price: 5)");
        let merge = merge_content(&files, &["hot", "cold"]);
        assert!(merge.errors.is_empty(), "{:?}", merge.errors);
        assert_eq!(merge.files, 4);
        assert_eq!(merge.db.move_def("ember").map(|m| m.power), Some(90));
        assert_eq!(merge.db.overrides().len(), 1);
        assert_eq!(merge.db.ids(ContentKind::Terrain), ["hot:ash", "lava"]);
        assert_eq!(
            merge.db.get(ContentKind::Item, "ice").map(|e| &e.source),
            Some(&ContentSource::Plugin("cold".into()))
        );
    }

    #[test]
    fn every_problem_is_collected() {
        let files = MemoryScripts::new()
            .with("data/moves/ember.ron", MOVE)
            .with("plugins/a/data/moves/ember.ron", MOVE)
            .with("plugins/a/data/moves/broken.ron", "(id: ")
            .with("plugins/a/data/terrains/water.ron", "(id: \"water\")")
            .with("plugins/a/data/items/x.ron", "(id: \"b:x\")");
        let merge = merge_content(&files, &["a"]);
        let messages: Vec<String> = merge.errors.iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 4, "{messages:#?}");
        assert!(messages.iter().any(|m| m.starts_with(
            "plugins/a/data/moves/ember.ron: duplicate move `ember`: already defined by the game"
        )));
        assert!(messages.iter().any(|m| m.contains("broken.ron")));
        assert!(messages.iter().any(|m| m.contains("built-in terrain")));
        assert!(messages.iter().any(|m| m.contains("`b:` namespace")));
        // The game's record survived.
        assert_eq!(
            merge.db.get(ContentKind::Move, "ember").map(|e| &e.source),
            Some(&ContentSource::Game)
        );
    }
}
