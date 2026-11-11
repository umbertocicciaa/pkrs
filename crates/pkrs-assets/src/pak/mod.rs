//! The `.pak` release archive (§13): a zip container with a `pak.ron` index.
//!
//! The index ([`PakIndex`]) records the game id and version, the engine requirement and API
//! level the game was packed for, the file table (path, size, CRC-32 of every file), the
//! release texture atlases (§5.2) and a content hash over all of it. [`PakVfs`](crate::PakVfs)
//! checks the index against the archive when it opens, and the runtime checks the engine
//! version and API level before it starts the game ([`PakIndex::check_runtime`]).
//!
//! [`PakBuilder`](crate::PakBuilder) writes archives (used by `pkrs pack`). Packing is
//! deterministic: files are sorted, timestamps are fixed, so the same project always gives
//! the same bytes.
//!
//! The content hash detects corrupted or edited archives; it is not a security measure.

mod codec;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::ops::RangeInclusive;

use pkrs_core::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::path::VfsPath;

pub use codec::{index_from_ron, index_to_ron};

/// Path of the index inside the archive.
pub const PAK_INDEX: &str = "pak.ron";

/// The `.pak` format version this engine reads and writes.
pub const PAK_FORMAT: u32 = 1;

/// Prefix of [`PakIndex::content_hash`] values (the hash function used).
const HASH_PREFIX: &str = "fnv1a64:";

/// Why a `.pak` archive could not be written, opened or used.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum PakError {
    /// Reading or writing the archive file failed.
    #[error("cannot access `{path}`: {message}")]
    Io {
        /// The archive path.
        path: String,
        /// Description of the failure.
        message: String,
    },
    /// The file is not a readable zip archive.
    #[error("not a valid .pak archive: {0}")]
    Archive(String),
    /// The archive has no `pak.ron`.
    #[error("the archive has no `{PAK_INDEX}` index")]
    MissingIndex,
    /// `pak.ron` could not be parsed or is inconsistent.
    #[error("invalid `{PAK_INDEX}`: {0}")]
    Index(String),
    /// The archive was written in a format this engine does not read.
    #[error("unsupported .pak format {found} (this engine reads format {supported})")]
    UnsupportedFormat {
        /// Format in the index.
        found: u32,
        /// Format this engine reads.
        supported: u32,
    },
    /// The index's content hash does not match its contents.
    #[error("content hash mismatch: the index says `{expected}`, the contents hash to `{actual}`")]
    ContentHashMismatch {
        /// Hash stored in the index.
        expected: String,
        /// Hash computed when opening.
        actual: String,
    },
    /// The archive does not match the index's file table.
    #[error("file `{path}` {problem}")]
    FileTable {
        /// The file that differs.
        path: String,
        /// How it differs.
        problem: String,
    },
    /// The game needs a different engine version.
    #[error("the game requires engine `{requirement}`, this is engine {engine}")]
    EngineMismatch {
        /// `engine.version` the game was packed with.
        requirement: String,
        /// This engine's version.
        engine: String,
    },
    /// The game targets a scripting API level this engine does not support.
    #[error(
        "the game targets scripting API level {api}, this engine supports {}..={}",
        supported.start(),
        supported.end()
    )]
    ApiMismatch {
        /// `engine.api` the game was packed with.
        api: u32,
        /// Levels this engine supports.
        supported: RangeInclusive<u32>,
    },
    /// A path given to the builder is reserved, duplicated or invalid.
    #[error("cannot add `{path}`: {reason}")]
    InvalidEntry {
        /// The rejected path.
        path: String,
        /// Why.
        reason: String,
    },
}

/// One file of the archive (atlas pages included, `pak.ron` excluded).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakFile {
    /// Project-relative path.
    pub path: String,
    /// Uncompressed size in bytes.
    pub size: u64,
    /// CRC-32 (IEEE) of the contents, as zip stores it.
    pub crc32: u32,
}

/// One texture atlas page (a PNG file in the archive).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasPage {
    /// Path of the page image in the archive (`atlases/tilesets_0.png`).
    pub path: String,
    /// Atlas category (`characters`, `tilesets`, …).
    pub category: String,
    /// Page width in pixels.
    pub width: u32,
    /// Page height in pixels.
    pub height: u32,
}

/// Where a packed image lives: a page of [`PakIndex::atlases`] and a rectangle on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AtlasRect {
    /// Index into [`PakIndex::atlases`].
    pub page: u32,
    /// Left edge in pixels.
    pub x: u32,
    /// Top edge in pixels.
    pub y: u32,
    /// Width in pixels (the original image's width).
    pub width: u32,
    /// Height in pixels (the original image's height).
    pub height: u32,
}

/// The `pak.ron` index (§13).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PakIndex {
    /// Archive format ([`PAK_FORMAT`]).
    pub format: u32,
    /// `game.id` from `pkrs.toml`.
    pub game_id: String,
    /// `game.version` from `pkrs.toml`.
    pub game_version: String,
    /// `engine.version` requirement from `pkrs.toml`.
    pub engine: String,
    /// `engine.api` from `pkrs.toml`.
    pub api: u32,
    /// Version of the tool that wrote the archive (informational).
    pub packed_by: String,
    /// [`content_hash`](Self::compute_content_hash) of the table, atlases and rects.
    pub content_hash: String,
    /// Every file in the archive except `pak.ron`, sorted by path.
    pub files: Vec<PakFile>,
    /// Atlas pages, in page order.
    pub atlases: Vec<AtlasPage>,
    /// Packed images: original project path → rectangle on an atlas page. These images are
    /// not stored as files of their own.
    pub sprites: BTreeMap<String, AtlasRect>,
}

/// Game and engine facts recorded in the index (from `pkrs.toml`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PakMeta {
    /// `game.id`.
    pub game_id: String,
    /// `game.version`.
    pub game_version: String,
    /// `engine.version` requirement.
    pub engine: String,
    /// `engine.api`.
    pub api: u32,
    /// Version of the packing tool.
    pub packed_by: String,
}

/// An image resolved through the atlas: its page and where it sits on the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AtlasLookup<'a> {
    /// The page holding the image.
    pub page: &'a AtlasPage,
    /// The image's rectangle on the page.
    pub rect: AtlasRect,
}

impl PakIndex {
    /// The atlas page and rectangle of the packed image originally at `path`, if it was
    /// packed.
    #[must_use]
    pub fn atlas_rect(&self, path: &VfsPath) -> Option<AtlasLookup<'_>> {
        let rect = *self.sprites.get(path.as_str())?;
        let page = self.atlases.get(rect.page as usize)?;
        Some(AtlasLookup { page, rect })
    }

    /// Hash over the file table, atlas pages and rects (not over itself or the metadata).
    #[must_use]
    pub fn compute_content_hash(&self) -> String {
        let mut h = Fnv64::new();
        for file in &self.files {
            h.str(&file.path);
            h.bytes(&file.size.to_le_bytes());
            h.bytes(&file.crc32.to_le_bytes());
        }
        h.bytes(b"\xffatlases");
        for page in &self.atlases {
            h.str(&page.path);
            h.str(&page.category);
            h.bytes(&page.width.to_le_bytes());
            h.bytes(&page.height.to_le_bytes());
        }
        h.bytes(b"\xffsprites");
        for (path, r) in &self.sprites {
            h.str(path);
            for v in [r.page, r.x, r.y, r.width, r.height] {
                h.bytes(&v.to_le_bytes());
            }
        }
        format!("{HASH_PREFIX}{:016x}", h.finish())
    }

    /// Checks the index on its own: format, content hash, paths, and that every atlas rect
    /// fits on an existing page that is a file of the archive.
    pub fn validate(&self) -> Result<(), PakError> {
        if self.format != PAK_FORMAT {
            return Err(PakError::UnsupportedFormat {
                found: self.format,
                supported: PAK_FORMAT,
            });
        }
        let actual = self.compute_content_hash();
        if actual != self.content_hash {
            return Err(PakError::ContentHashMismatch {
                expected: self.content_hash.clone(),
                actual,
            });
        }
        let mut seen = BTreeSet::new();
        for file in &self.files {
            check_path(&file.path)?;
            if file.path == PAK_INDEX {
                return Err(PakError::Index(format!(
                    "`{PAK_INDEX}` is listed as a file"
                )));
            }
            if !seen.insert(file.path.as_str()) {
                return Err(PakError::Index(format!("`{}` is listed twice", file.path)));
            }
        }
        for page in &self.atlases {
            check_path(&page.path)?;
            if !seen.contains(page.path.as_str()) {
                return Err(PakError::Index(format!(
                    "atlas page `{}` is not a file of the archive",
                    page.path
                )));
            }
        }
        for (path, r) in &self.sprites {
            check_path(path)?;
            if seen.contains(path.as_str()) {
                return Err(PakError::Index(format!(
                    "`{path}` is both a file and a packed image"
                )));
            }
            let page = self.atlases.get(r.page as usize).ok_or_else(|| {
                PakError::Index(format!(
                    "`{path}` is on atlas page {}, which does not exist",
                    r.page
                ))
            })?;
            let fits = r.width > 0
                && r.height > 0
                && r.x
                    .checked_add(r.width)
                    .is_some_and(|right| right <= page.width)
                && r.y
                    .checked_add(r.height)
                    .is_some_and(|bottom| bottom <= page.height);
            if !fits {
                return Err(PakError::Index(format!(
                    "`{path}` ({}x{} at {},{}) does not fit on `{}` ({}x{})",
                    r.width, r.height, r.x, r.y, page.path, page.width, page.height
                )));
            }
        }
        Ok(())
    }

    /// Checks that this engine can run the game: `engine` must satisfy the packed engine
    /// requirement and the API level must be in `supported_api` (§2.5, §9.7).
    pub fn check_runtime(
        &self,
        engine: &Version,
        supported_api: RangeInclusive<u32>,
    ) -> Result<(), PakError> {
        let requirement = VersionReq::parse(&self.engine)
            .map_err(|e| PakError::Index(format!("engine requirement `{}`: {e}", self.engine)))?;
        if !requirement.matches(engine) {
            return Err(PakError::EngineMismatch {
                requirement: self.engine.clone(),
                engine: engine.to_string(),
            });
        }
        if !supported_api.contains(&self.api) {
            return Err(PakError::ApiMismatch {
                api: self.api,
                supported: supported_api,
            });
        }
        Ok(())
    }
}

impl fmt::Display for PakIndex {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} ({} files, {} atlas pages, {} packed images)",
            self.game_id,
            self.game_version,
            self.files.len(),
            self.atlases.len(),
            self.sprites.len()
        )
    }
}

fn check_path(raw: &str) -> Result<(), PakError> {
    match VfsPath::new(raw) {
        Ok(p) if p.as_str() == raw && !p.is_root() => Ok(()),
        Ok(_) => Err(PakError::Index(format!("path `{raw}` is not normalised"))),
        Err(e) => Err(PakError::Index(e.to_string())),
    }
}

/// 64-bit FNV-1a; strings are length-prefixed so `("ab","c")` and `("a","bc")` differ.
struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }

    fn bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn str(&mut self, s: &str) {
        self.bytes(&(s.len() as u64).to_le_bytes());
        self.bytes(s.as_bytes());
    }

    fn finish(&self) -> u64 {
        self.0
    }
}

/// CRC-32 (IEEE 802.3, as used by zip) of `bytes`.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0u32; 256];
        let mut i = 0;
        while i < 256 {
            let mut c = i as u32;
            let mut k = 0;
            while k < 8 {
                c = if c & 1 != 0 {
                    0xedb8_8320 ^ (c >> 1)
                } else {
                    c >> 1
                };
                k += 1;
            }
            table[i] = c;
            i += 1;
        }
        table
    };
    let mut crc = !0u32;
    for &b in bytes {
        crc = TABLE[((crc ^ u32::from(b)) & 0xff) as usize] ^ (crc >> 8);
    }
    !crc
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    pub(crate) fn sample_index() -> PakIndex {
        let mut index = PakIndex {
            format: PAK_FORMAT,
            game_id: "com.example.town".into(),
            game_version: "0.1.0".into(),
            engine: "0.0".into(),
            api: 1,
            packed_by: "0.0.0".into(),
            content_hash: String::new(),
            files: vec![
                PakFile {
                    path: "atlases/tilesets_0.png".into(),
                    size: 120,
                    crc32: 7,
                },
                PakFile {
                    path: "pkrs.toml".into(),
                    size: 40,
                    crc32: 9,
                },
            ],
            atlases: vec![AtlasPage {
                path: "atlases/tilesets_0.png".into(),
                category: "tilesets".into(),
                width: 64,
                height: 32,
            }],
            sprites: BTreeMap::from([(
                "tilesets/overworld.png".to_owned(),
                AtlasRect {
                    page: 0,
                    x: 16,
                    y: 0,
                    width: 32,
                    height: 32,
                },
            )]),
        };
        index.content_hash = index.compute_content_hash();
        index
    }

    #[test]
    fn crc32_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn sample_index_is_valid() {
        sample_index().validate().unwrap();
    }

    #[test]
    fn atlas_rect_lookup_finds_packed_images_only() {
        let index = sample_index();
        let found = index
            .atlas_rect(&VfsPath::new("tilesets/overworld.png").unwrap())
            .unwrap();
        assert_eq!(found.page.path, "atlases/tilesets_0.png");
        assert_eq!((found.rect.x, found.rect.y, found.rect.width), (16, 0, 32));
        assert!(
            index
                .atlas_rect(&VfsPath::new("pkrs.toml").unwrap())
                .is_none()
        );
        assert!(
            index
                .atlas_rect(&VfsPath::new("tilesets/none.png").unwrap())
                .is_none()
        );
    }

    #[test]
    fn content_hash_covers_files_and_rects() {
        let index = sample_index();
        let mut edited = index.clone();
        edited.files[1].size += 1;
        assert_ne!(edited.compute_content_hash(), index.content_hash);
        assert!(matches!(
            edited.validate(),
            Err(PakError::ContentHashMismatch { .. })
        ));
        let mut moved = index.clone();
        moved.sprites.values_mut().for_each(|r| r.x = 0);
        assert_ne!(moved.compute_content_hash(), index.content_hash);
        assert!(index.content_hash.starts_with("fnv1a64:"));
    }

    #[test]
    fn validate_rejects_rects_off_the_page() {
        let mut index = sample_index();
        index.sprites.values_mut().for_each(|r| r.x = 40);
        index.content_hash = index.compute_content_hash();
        let err = index.validate().unwrap_err().to_string();
        assert!(err.contains("does not fit"), "{err}");

        let mut index = sample_index();
        index.sprites.values_mut().for_each(|r| r.page = 3);
        index.content_hash = index.compute_content_hash();
        assert!(index.validate().unwrap_err().to_string().contains("page 3"));
    }

    #[test]
    fn validate_rejects_unknown_formats() {
        let mut index = sample_index();
        index.format = 99;
        assert!(matches!(
            index.validate(),
            Err(PakError::UnsupportedFormat {
                found: 99,
                supported: PAK_FORMAT
            })
        ));
    }

    #[test]
    fn check_runtime_reports_engine_and_api_mismatches() {
        let index = sample_index();
        let engine = Version::parse("0.0.5").unwrap();
        index.check_runtime(&engine, 1..=1).unwrap();

        let newer = Version::parse("1.2.0").unwrap();
        let err = index.check_runtime(&newer, 1..=1).unwrap_err();
        assert!(matches!(err, PakError::EngineMismatch { .. }));
        assert_eq!(
            err.to_string(),
            "the game requires engine `0.0`, this is engine 1.2.0"
        );

        let err = index.check_runtime(&engine, 2..=3).unwrap_err();
        assert_eq!(
            err.to_string(),
            "the game targets scripting API level 1, this engine supports 2..=3"
        );
    }
}
