//! `PakVfs` and `PakBuilder`: the zip adapter for `.pak` archives (§4.5, §13). This is the
//! only module of `pkrs-assets` that uses the `zip` crate.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs::File;
use std::io::{BufReader, Cursor, Read, Seek, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, DateTime, ZipArchive, ZipWriter};

use super::{EntryKind, Vfs, VfsEntry};
use crate::error::{AssetsError, Result};
use crate::pak::{
    AtlasPage, AtlasRect, PAK_FORMAT, PAK_INDEX, PakError, PakFile, PakIndex, PakMeta, crc32,
    index_from_ron, index_to_ron,
};
use crate::path::VfsPath;

/// Anything an archive can be read from.
trait Source: Read + Seek + Send {}
impl<T: Read + Seek + Send> Source for T {}

struct Inner {
    archive: Mutex<ZipArchive<Box<dyn Source>>>,
    index: PakIndex,
    files: BTreeSet<VfsPath>,
    dirs: BTreeSet<VfsPath>,
    origin: String,
}

/// A read-only [`Vfs`] over a `.pak` archive (release builds, §13).
///
/// Opening checks the `pak.ron` index (format, content hash, atlas rects) and that the
/// archive holds exactly the files of the index's table with the recorded sizes and CRC-32s;
/// every read is CRC-checked again by the zip reader. Images packed into atlases are not
/// files of their own: look them up with [`PakIndex::atlas_rect`] through
/// [`index`](Self::index).
///
/// Cloning is cheap (the archive is shared); reads are serialised on one file handle.
#[derive(Clone)]
pub struct PakVfs {
    inner: Arc<Inner>,
}

impl fmt::Debug for PakVfs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PakVfs")
            .field("origin", &self.inner.origin)
            .field("index", &self.inner.index.to_string())
            .finish()
    }
}

impl PakVfs {
    /// Opens and checks the archive at `path`.
    pub fn open(path: &Path) -> std::result::Result<Self, PakError> {
        let file = File::open(path).map_err(|e| PakError::Io {
            path: path.display().to_string(),
            message: e.to_string(),
        })?;
        Self::from_source(Box::new(BufReader::new(file)), path.display().to_string())
    }

    /// Opens and checks an archive held in memory (tests, embedded data).
    pub fn from_bytes(bytes: Vec<u8>) -> std::result::Result<Self, PakError> {
        Self::from_source(Box::new(Cursor::new(bytes)), "<memory>".to_owned())
    }

    fn from_source(source: Box<dyn Source>, origin: String) -> std::result::Result<Self, PakError> {
        let mut archive = ZipArchive::new(source).map_err(|e| PakError::Archive(e.to_string()))?;
        let index = read_index(&mut archive)?;
        index.validate()?;
        let actual = archive_table(&mut archive)?;
        check_table(&index.files, &actual)?;

        let mut files = BTreeSet::new();
        let mut dirs = BTreeSet::new();
        for file in &index.files {
            let path = VfsPath::new(&file.path).map_err(|e| PakError::Index(e.to_string()))?;
            let mut dir = String::new();
            let segments: Vec<&str> = path.segments().collect();
            for segment in &segments[..segments.len().saturating_sub(1)] {
                if !dir.is_empty() {
                    dir.push('/');
                }
                dir.push_str(segment);
                dirs.insert(VfsPath::new(&dir).map_err(|e| PakError::Index(e.to_string()))?);
            }
            files.insert(path);
        }
        Ok(Self {
            inner: Arc::new(Inner {
                archive: Mutex::new(archive),
                index,
                files,
                dirs,
                origin,
            }),
        })
    }

    /// The archive's checked `pak.ron` index.
    #[must_use]
    pub fn index(&self) -> &PakIndex {
        &self.inner.index
    }

    fn case_insensitive_match(&self, path: &VfsPath) -> Option<&VfsPath> {
        let wanted = path.as_str().to_lowercase();
        self.inner
            .files
            .iter()
            .find(|p| p.as_str().to_lowercase() == wanted)
    }
}

fn read_index(
    archive: &mut ZipArchive<Box<dyn Source>>,
) -> std::result::Result<PakIndex, PakError> {
    let mut entry = match archive.by_name(PAK_INDEX) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Err(PakError::MissingIndex),
        Err(e) => return Err(PakError::Archive(e.to_string())),
    };
    let mut text = String::new();
    entry
        .read_to_string(&mut text)
        .map_err(|e| PakError::Index(e.to_string()))?;
    index_from_ron(&text)
}

/// `(size, crc32)` of every file entry in the archive except the index.
fn archive_table(
    archive: &mut ZipArchive<Box<dyn Source>>,
) -> std::result::Result<BTreeMap<String, (u64, u32)>, PakError> {
    let mut table = BTreeMap::new();
    for i in 0..archive.len() {
        let entry = archive
            .by_index_raw(i)
            .map_err(|e| PakError::Archive(e.to_string()))?;
        let name = entry
            .name()
            .map_err(|e| PakError::Archive(e.to_string()))?
            .into_owned();
        if entry.is_dir() || name == PAK_INDEX {
            continue;
        }
        table.insert(name, (entry.size(), entry.crc32()));
    }
    Ok(table)
}

fn check_table(
    expected: &[PakFile],
    actual: &BTreeMap<String, (u64, u32)>,
) -> std::result::Result<(), PakError> {
    for file in expected {
        match actual.get(&file.path) {
            None => {
                return Err(PakError::FileTable {
                    path: file.path.clone(),
                    problem: "is in the index but missing from the archive".into(),
                });
            }
            Some(&(size, crc)) if size != file.size || crc != file.crc32 => {
                return Err(PakError::FileTable {
                    path: file.path.clone(),
                    problem: format!(
                        "differs from the index (size {size}, crc32 {crc:08x}; expected size {}, crc32 {:08x})",
                        file.size, file.crc32
                    ),
                });
            }
            Some(_) => {}
        }
    }
    if actual.len() != expected.len() {
        let listed: BTreeSet<&str> = expected.iter().map(|f| f.path.as_str()).collect();
        if let Some(extra) = actual.keys().find(|p| !listed.contains(p.as_str())) {
            return Err(PakError::FileTable {
                path: extra.clone(),
                problem: "is in the archive but not in the index".into(),
            });
        }
    }
    Ok(())
}

impl Vfs for PakVfs {
    fn read(&self, path: &VfsPath) -> Result<Vec<u8>> {
        if !self.inner.files.contains(path) {
            return match self.case_insensitive_match(path) {
                Some(actual) => Err(AssetsError::CaseMismatch {
                    requested: path.to_string(),
                    actual: actual.to_string(),
                }),
                None => Err(AssetsError::NotFound(path.to_string())),
            };
        }
        let io = |message: String| AssetsError::Io {
            path: path.to_string(),
            message,
        };
        let mut archive = self
            .inner
            .archive
            .lock()
            .map_err(|_| io("the archive reader was poisoned by an earlier panic".into()))?;
        let mut entry = archive
            .by_name(path.as_str())
            .map_err(|e| io(e.to_string()))?;
        let mut bytes = Vec::with_capacity(usize::try_from(entry.size()).unwrap_or(0));
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| io(e.to_string()))?;
        Ok(bytes)
    }

    fn exists(&self, path: &VfsPath) -> bool {
        self.inner.files.contains(path)
    }

    fn list(&self, dir: &VfsPath) -> Result<Vec<VfsEntry>> {
        if !dir.is_root() && !self.inner.dirs.contains(dir) {
            return Err(AssetsError::NotFound(dir.to_string()));
        }
        let children = |set: &BTreeSet<VfsPath>, kind: EntryKind| {
            set.iter()
                .filter(|p| {
                    p.starts_with_dir(dir) && p.segments().count() == dir.segments().count() + 1
                })
                .map(|p| VfsEntry {
                    path: p.clone(),
                    kind,
                })
                .collect::<Vec<_>>()
        };
        let mut entries = children(&self.inner.dirs, EntryKind::Dir);
        entries.extend(children(&self.inner.files, EntryKind::File));
        entries.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(entries)
    }
}

/// Writes `.pak` archives: add files, atlas pages and packed-image rects, then
/// [`build`](Self::build) or [`write_to`](Self::write_to).
#[derive(Debug)]
pub struct PakBuilder {
    meta: PakMeta,
    files: BTreeMap<VfsPath, Vec<u8>>,
    atlases: Vec<AtlasPage>,
    sprites: BTreeMap<String, AtlasRect>,
}

impl PakBuilder {
    /// An empty archive for the game described by `meta`.
    #[must_use]
    pub fn new(meta: PakMeta) -> Self {
        Self {
            meta,
            files: BTreeMap::new(),
            atlases: Vec::new(),
            sprites: BTreeMap::new(),
        }
    }

    /// Adds a file. `pak.ron` is reserved and each path may be added once.
    pub fn add_file(&mut self, path: VfsPath, bytes: Vec<u8>) -> std::result::Result<(), PakError> {
        let reject = |reason: &str| PakError::InvalidEntry {
            path: path.to_string(),
            reason: reason.to_owned(),
        };
        if path.is_root() {
            return Err(reject("a file needs a name"));
        }
        if path.as_str() == PAK_INDEX {
            return Err(reject("the name is reserved for the index"));
        }
        if self.files.contains_key(&path) || self.sprites.contains_key(path.as_str()) {
            return Err(reject("it is already in the archive"));
        }
        self.files.insert(path, bytes);
        Ok(())
    }

    /// Adds an atlas page image stored at `page.path`; returns its page number.
    pub fn add_atlas_page(
        &mut self,
        page: AtlasPage,
        png: Vec<u8>,
    ) -> std::result::Result<u32, PakError> {
        let path = VfsPath::new(&page.path).map_err(|e| PakError::InvalidEntry {
            path: page.path.clone(),
            reason: e.to_string(),
        })?;
        let number = u32::try_from(self.atlases.len()).map_err(|_| PakError::InvalidEntry {
            path: page.path.clone(),
            reason: "too many atlas pages".into(),
        })?;
        self.add_file(path, png)?;
        self.atlases.push(page);
        Ok(number)
    }

    /// Records that the image originally at `path` is packed at `rect`.
    pub fn add_packed_image(
        &mut self,
        path: &VfsPath,
        rect: AtlasRect,
    ) -> std::result::Result<(), PakError> {
        if self.files.contains_key(path) || self.sprites.contains_key(path.as_str()) {
            return Err(PakError::InvalidEntry {
                path: path.to_string(),
                reason: "it is already in the archive".into(),
            });
        }
        self.sprites.insert(path.to_string(), rect);
        Ok(())
    }

    /// The index this builder will write (content hash included).
    #[must_use]
    pub fn index(&self) -> PakIndex {
        let mut index = PakIndex {
            format: PAK_FORMAT,
            game_id: self.meta.game_id.clone(),
            game_version: self.meta.game_version.clone(),
            engine: self.meta.engine.clone(),
            api: self.meta.api,
            packed_by: self.meta.packed_by.clone(),
            content_hash: String::new(),
            files: self
                .files
                .iter()
                .map(|(path, bytes)| PakFile {
                    path: path.to_string(),
                    size: bytes.len() as u64,
                    crc32: crc32(bytes),
                })
                .collect(),
            atlases: self.atlases.clone(),
            sprites: self.sprites.clone(),
        };
        index.content_hash = index.compute_content_hash();
        index
    }

    /// The archive bytes and their index. Fails if the index would not validate.
    pub fn build(&self) -> std::result::Result<(PakIndex, Vec<u8>), PakError> {
        let index = self.index();
        index.validate()?;
        let zipped = |e: zip::result::ZipError| PakError::Archive(e.to_string());
        let io = |e: std::io::Error| PakError::Archive(e.to_string());
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let options = |method| {
            SimpleFileOptions::default()
                .compression_method(method)
                .last_modified_time(DateTime::DEFAULT)
                .unix_permissions(0o644)
        };
        zip.start_file(PAK_INDEX, options(CompressionMethod::Deflated))
            .map_err(zipped)?;
        zip.write_all(index_to_ron(&index)?.as_bytes())
            .map_err(io)?;
        for (path, bytes) in &self.files {
            zip.start_file(path.as_str(), options(compression_for(path)))
                .map_err(zipped)?;
            zip.write_all(bytes).map_err(io)?;
        }
        let bytes = zip.finish().map_err(zipped)?.into_inner();
        Ok((index, bytes))
    }

    /// Builds the archive and writes it to `out` (through a temporary file, so a failed
    /// write never leaves a truncated archive behind).
    pub fn write_to(&self, out: &Path) -> std::result::Result<PakIndex, PakError> {
        let (index, bytes) = self.build()?;
        let io = |e: std::io::Error| PakError::Io {
            path: out.display().to_string(),
            message: e.to_string(),
        };
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(io)?;
        }
        let mut tmp = out.as_os_str().to_owned();
        tmp.push(".tmp");
        let tmp = std::path::PathBuf::from(tmp);
        std::fs::write(&tmp, &bytes).map_err(io)?;
        if let Err(err) = std::fs::rename(&tmp, out) {
            let _ = std::fs::remove_file(&tmp);
            return Err(io(err));
        }
        Ok(index)
    }
}

/// Already-compressed formats are stored; everything else is deflated.
fn compression_for(path: &VfsPath) -> CompressionMethod {
    let ext = path
        .file_name()
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png" | "ogg" | "jpg" | "jpeg" | "zip") => CompressionMethod::Stored,
        _ => CompressionMethod::Deflated,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    fn meta() -> PakMeta {
        PakMeta {
            game_id: "com.example.town".into(),
            game_version: "0.1.0".into(),
            engine: "0.0".into(),
            api: 1,
            packed_by: "0.0.0".into(),
        }
    }

    fn p(raw: &str) -> VfsPath {
        VfsPath::new(raw).unwrap()
    }

    fn sample() -> PakBuilder {
        let mut b = PakBuilder::new(meta());
        b.add_file(p("pkrs.toml"), b"[game]\n".to_vec()).unwrap();
        b.add_file(p("maps/town.tmx"), b"<map/>".to_vec()).unwrap();
        b.add_file(p("scripts/maps/town.lua"), b"-- town".to_vec())
            .unwrap();
        let page = b
            .add_atlas_page(
                AtlasPage {
                    path: "atlases/tilesets_0.png".into(),
                    category: "tilesets".into(),
                    width: 32,
                    height: 16,
                },
                vec![1, 2, 3],
            )
            .unwrap();
        b.add_packed_image(
            &p("tilesets/overworld.png"),
            AtlasRect {
                page,
                x: 0,
                y: 0,
                width: 16,
                height: 16,
            },
        )
        .unwrap();
        b
    }

    #[test]
    fn built_archive_reads_back_through_the_vfs() {
        let (index, bytes) = sample().build().unwrap();
        let pak = PakVfs::from_bytes(bytes).unwrap();
        assert_eq!(pak.index(), &index);
        assert_eq!(pak.read(&p("maps/town.tmx")).unwrap(), b"<map/>");
        assert!(pak.exists(&p("scripts/maps/town.lua")));
        assert!(!pak.exists(&p("scripts")), "directories are not files");
        assert!(
            !pak.exists(&p(PAK_INDEX)),
            "the index is not a project file"
        );
        assert!(
            !pak.exists(&p("tilesets/overworld.png")),
            "packed images are not files"
        );
        assert!(
            pak.index()
                .atlas_rect(&p("tilesets/overworld.png"))
                .is_some()
        );
    }

    #[test]
    fn list_returns_direct_children_sorted() {
        let pak = PakVfs::from_bytes(sample().build().unwrap().1).unwrap();
        let root: Vec<_> = pak
            .list(&VfsPath::root())
            .unwrap()
            .into_iter()
            .map(|e| (e.path.to_string(), e.kind))
            .collect();
        assert_eq!(
            root,
            vec![
                ("atlases".into(), EntryKind::Dir),
                ("maps".into(), EntryKind::Dir),
                ("pkrs.toml".into(), EntryKind::File),
                ("scripts".into(), EntryKind::Dir),
            ]
        );
        let scripts = pak.list(&p("scripts")).unwrap();
        assert_eq!(scripts.len(), 1);
        assert_eq!(scripts[0].path, p("scripts/maps"));
        assert!(matches!(
            pak.list(&p("nope")),
            Err(AssetsError::NotFound(_))
        ));
    }

    #[test]
    fn reads_report_missing_files_and_case_mismatches() {
        let pak = PakVfs::from_bytes(sample().build().unwrap().1).unwrap();
        assert!(matches!(
            pak.read(&p("maps/none.tmx")),
            Err(AssetsError::NotFound(_))
        ));
        match pak.read(&p("Maps/Town.tmx")) {
            Err(AssetsError::CaseMismatch { actual, .. }) => assert_eq!(actual, "maps/town.tmx"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn packing_is_deterministic() {
        let a = sample().build().unwrap().1;
        let b = sample().build().unwrap().1;
        assert_eq!(a, b);
    }

    #[test]
    fn builder_rejects_reserved_and_duplicate_paths() {
        let mut b = sample();
        assert!(b.add_file(p(PAK_INDEX), vec![]).is_err());
        assert!(b.add_file(p("pkrs.toml"), vec![]).is_err());
        assert!(b.add_file(p("tilesets/overworld.png"), vec![]).is_err());
        assert!(
            b.add_packed_image(
                &p("maps/town.tmx"),
                AtlasRect {
                    page: 0,
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1
                }
            )
            .is_err()
        );
    }

    /// Rewrites `bytes` with `edit` applied to the entries (name, contents).
    fn rezip(bytes: Vec<u8>, edit: impl Fn(&str, Vec<u8>) -> Option<Vec<u8>>) -> Vec<u8> {
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut out = ZipWriter::new(Cursor::new(Vec::new()));
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i).unwrap();
            let name = entry.name().unwrap().into_owned();
            let mut data = Vec::new();
            entry.read_to_end(&mut data).unwrap();
            if let Some(data) = edit(&name, data) {
                out.start_file(name, SimpleFileOptions::default()).unwrap();
                out.write_all(&data).unwrap();
            }
        }
        out.finish().unwrap().into_inner()
    }

    #[test]
    fn open_detects_changed_missing_and_extra_files() {
        let bytes = sample().build().unwrap().1;
        let changed = rezip(bytes.clone(), |name, data| {
            Some(if name == "maps/town.tmx" {
                b"<map />".to_vec()
            } else {
                data
            })
        });
        let err = PakVfs::from_bytes(changed).unwrap_err();
        assert!(
            matches!(&err, PakError::FileTable { path, .. } if path == "maps/town.tmx"),
            "{err}"
        );

        let missing = rezip(bytes.clone(), |name, data| {
            (name != "pkrs.toml").then_some(data)
        });
        assert!(
            PakVfs::from_bytes(missing)
                .unwrap_err()
                .to_string()
                .contains("missing from the archive")
        );

        let no_index = rezip(bytes, |name, data| (name != PAK_INDEX).then_some(data));
        assert!(matches!(
            PakVfs::from_bytes(no_index),
            Err(PakError::MissingIndex)
        ));
        assert!(matches!(
            PakVfs::from_bytes(b"not a zip".to_vec()),
            Err(PakError::Archive(_))
        ));
    }

    #[test]
    fn open_detects_an_edited_index() {
        let bytes = sample().build().unwrap().1;
        let edited = rezip(bytes, |name, data| {
            Some(if name == PAK_INDEX {
                String::from_utf8(data)
                    .unwrap()
                    .replace("x: 0,", "x: 16,")
                    .into_bytes()
            } else {
                data
            })
        });
        assert!(matches!(
            PakVfs::from_bytes(edited),
            Err(PakError::ContentHashMismatch { .. })
        ));
    }

    #[test]
    fn write_to_creates_the_file_atomically() {
        let dir = std::env::temp_dir().join(format!("pkrs-pak-{}", std::process::id()));
        let out = dir.join("dist/game.pak");
        let index = sample().write_to(&out).unwrap();
        let pak = PakVfs::open(&out).unwrap();
        assert_eq!(pak.index(), &index);
        assert!(!dir.join("dist/game.pak.tmp").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
