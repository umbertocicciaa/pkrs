//! `pkrs pack`: packs a project folder into a `game.pak` release archive (§13, §5.2).
//!
//! Every project file goes into the archive except dev-only folders (`saves/`, `dist/`,
//! `tests/`, `target/`), hidden files and folders (`.git`, `.gitignore`, …) and existing
//! `.pak` archives. PNG images under the atlas folders ([`ATLAS_CATEGORIES`]) are packed into
//! one set of atlas pages per category through the [`AtlasPacker`] port and their rects are
//! recorded in `pak.ron`; the runtime draws them from the pages without any visible
//! difference. Images that do not fit on a page stay files of their own.

use std::fmt;
use std::path::{Path, PathBuf};

use pkrs_assets::{
    AtlasPage, AtlasRect, EntryKind, FolderVfs, PakBuilder, PakError, PakIndex, PakMeta, Vfs,
    VfsPath,
};
use pkrs_core::ProjectManifest;

use crate::atlas::{AtlasError, AtlasLimits, AtlasPacker, AtlasSource, Unpacked};

/// Default archive name (§14.1).
pub const PAK_FILE: &str = "game.pak";

/// Project folders packed into atlases, and the category (atlas name) of each.
pub const ATLAS_CATEGORIES: &[(&str, &str)] =
    &[("sprites", "characters"), ("tilesets", "tilesets")];

/// Top-level folders that never ship.
const EXCLUDED_DIRS: &[&str] = &["saves", "dist", "tests", "target"];

/// Folder of the atlas pages inside the archive.
const ATLAS_DIR: &str = "atlases";

/// Why packing failed.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// `pkrs.toml` is missing or invalid.
    #[error(transparent)]
    Manifest(#[from] pkrs_core::ManifestError),
    /// A project file could not be read.
    #[error(transparent)]
    Assets(#[from] pkrs_assets::AssetsError),
    /// The archive could not be built or written.
    #[error(transparent)]
    Pak(#[from] PakError),
    /// The atlas packer failed.
    #[error(transparent)]
    Atlas(#[from] AtlasError),
}

/// What was packed.
#[derive(Clone, Debug)]
pub struct PackReport {
    /// Where the archive was written.
    pub out: PathBuf,
    /// The archive's index.
    pub index: PakIndex,
    /// Images under an atlas folder that stayed files of their own.
    pub unpacked: Vec<Unpacked>,
}

impl fmt::Display for PackReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Packed {} into {}", self.index, self.out.display())?;
        for u in &self.unpacked {
            write!(f, "\n  not in an atlas: {} ({})", u.key, u.reason)?;
        }
        Ok(())
    }
}

/// The default output path: `<project>/dist/game.pak`.
#[must_use]
pub fn default_out(project: &Path) -> PathBuf {
    project.join("dist").join(PAK_FILE)
}

/// Packs the project in `project` into `out` with `packer`.
pub fn pack_project(
    project: &Path,
    out: &Path,
    packer: &dyn AtlasPacker,
) -> Result<PackReport, PackError> {
    let (builder, unpacked) = build_pak(project, packer)?;
    let index = builder.write_to(out)?;
    Ok(PackReport {
        out: out.to_path_buf(),
        index,
        unpacked,
    })
}

/// Collects the project into a [`PakBuilder`] (atlases included) without writing it.
pub fn build_pak(
    project: &Path,
    packer: &dyn AtlasPacker,
) -> Result<(PakBuilder, Vec<Unpacked>), PackError> {
    let manifest = ProjectManifest::load(project)?;
    let vfs = FolderVfs::new(project)?;
    let mut builder = PakBuilder::new(PakMeta {
        game_id: manifest.game.id.clone(),
        game_version: manifest.game.version.to_string(),
        engine: manifest.engine.version.to_string(),
        api: manifest.engine.api,
        packed_by: env!("CARGO_PKG_VERSION").to_owned(),
    });

    let mut files = Vec::new();
    collect_files(&vfs, &VfsPath::root(), &mut files)?;

    let mut atlas_sources: Vec<Vec<AtlasSource>> = vec![Vec::new(); ATLAS_CATEGORIES.len()];
    for path in files {
        let bytes = vfs.read(&path)?;
        match atlas_category(&path) {
            Some(category) => atlas_sources[category].push(AtlasSource {
                key: path.to_string(),
                png: bytes,
            }),
            None => builder.add_file(path, bytes)?,
        }
    }

    let mut unpacked = Vec::new();
    for ((_, category), sources) in ATLAS_CATEGORIES.iter().zip(atlas_sources) {
        if sources.is_empty() {
            continue;
        }
        let atlas = packer.pack(&sources, AtlasLimits::default())?;
        let mut page_numbers = Vec::with_capacity(atlas.pages.len());
        for (i, page) in atlas.pages.into_iter().enumerate() {
            let number = builder.add_atlas_page(
                AtlasPage {
                    path: format!("{ATLAS_DIR}/{category}_{i}.png"),
                    category: (*category).to_owned(),
                    width: page.width,
                    height: page.height,
                },
                page.png,
            )?;
            page_numbers.push(number);
        }
        for source in sources {
            let path = VfsPath::new(&source.key)?;
            match atlas.rects.get(&source.key) {
                Some(r) => {
                    let page = page_numbers.get(r.page).copied().ok_or_else(|| {
                        AtlasError(format!("`{}` is on a page that does not exist", source.key))
                    })?;
                    builder.add_packed_image(
                        &path,
                        AtlasRect {
                            page,
                            x: r.x,
                            y: r.y,
                            width: r.width,
                            height: r.height,
                        },
                    )?;
                }
                None => builder.add_file(path, source.png)?,
            }
        }
        unpacked.extend(atlas.unpacked);
    }
    Ok((builder, unpacked))
}

/// Every shipped file below `dir`, depth first in path order.
fn collect_files(vfs: &FolderVfs, dir: &VfsPath, out: &mut Vec<VfsPath>) -> Result<(), PackError> {
    for entry in vfs.list(dir)? {
        if !ships(&entry.path, entry.kind) {
            continue;
        }
        match entry.kind {
            EntryKind::Dir => collect_files(vfs, &entry.path, out)?,
            EntryKind::File => out.push(entry.path),
        }
    }
    Ok(())
}

/// Whether `path` belongs in the release archive.
fn ships(path: &VfsPath, kind: EntryKind) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    if name.starts_with('.') {
        return false;
    }
    let top_level = path.segments().count() == 1;
    match kind {
        EntryKind::Dir => !(top_level && EXCLUDED_DIRS.contains(&name)),
        EntryKind::File => {
            let lower = name.to_ascii_lowercase();
            !(lower.ends_with(".pak") || lower.ends_with(".pak.tmp"))
        }
    }
}

/// The [`ATLAS_CATEGORIES`] index of a PNG under an atlas folder.
fn atlas_category(path: &VfsPath) -> Option<usize> {
    let is_png = path
        .file_name()
        .is_some_and(|n| n.to_ascii_lowercase().ends_with(".png"));
    if !is_png {
        return None;
    }
    let top = path.segments().next()?;
    ATLAS_CATEGORIES.iter().position(|(dir, _)| *dir == top)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn p(raw: &str) -> VfsPath {
        VfsPath::new(raw).unwrap()
    }

    #[test]
    fn dev_folders_hidden_files_and_paks_do_not_ship() {
        assert!(ships(&p("maps"), EntryKind::Dir));
        assert!(ships(&p("maps/town.tmx"), EntryKind::File));
        for dir in ["saves", "dist", "tests", "target", ".git"] {
            assert!(!ships(&p(dir), EntryKind::Dir), "{dir}");
        }
        assert!(ships(&p("data/tests"), EntryKind::Dir), "only top-level");
        assert!(!ships(&p(".gitignore"), EntryKind::File));
        assert!(!ships(&p("old/game.PAK"), EntryKind::File));
        assert!(!ships(&p("game.pak.tmp"), EntryKind::File));
    }

    #[test]
    fn only_pngs_under_atlas_folders_are_packed() {
        assert_eq!(atlas_category(&p("sprites/player.png")), Some(0));
        assert_eq!(atlas_category(&p("sprites/npcs/guide.PNG")), Some(0));
        assert_eq!(atlas_category(&p("tilesets/overworld.png")), Some(1));
        assert_eq!(atlas_category(&p("tilesets/overworld.tsx")), None);
        assert_eq!(atlas_category(&p("ui/skin.png")), None);
        assert_eq!(atlas_category(&p("plugins/x/sprites/a.png")), None);
    }
}
