//! The audio manifest (§11): `audio/bgm/index.ron` (or `assets/audio/bgm/index.ron`).
//!
//! ```ron
//! (
//!     bgm: {
//!         // Loops from sample 88200 to the end of the file after a one-time intro.
//!         "town": (file: "town.ogg", loop_start: 88200),
//!         // A separate intro file played once, then `battle_wild.ogg` loops.
//!         "battle_wild": (intro: "battle_wild_intro.ogg", loop_end: 2116800),
//!     },
//!     jingles: {
//!         "item_get": (mode: Duck),
//!         "victory_wild": (mode: Pause),
//!     },
//!     sfx: {
//!         "bump": (file: "bump.wav", max_instances: 1),
//!     },
//!     battle: (wild: "battle_wild", trainer: "battle_trainer", victory: "victory_wild"),
//! )
//! ```
//!
//! Every section and field is optional. A sound without an entry uses its defaults: BGM and
//! jingle files are `<root>/bgm/<id>.ogg`, SFX files `<root>/sfx/<id>.ogg` (or the `.wav`
//! with that stem when only one exists), BGM loops over the whole file, jingles duck the BGM
//! and SFX allow [`AudioConfig::default_sfx_instances`](crate::AudioConfig) instances.

use std::collections::BTreeMap;

use pkrs_assets::{EntryKind, Vfs, VfsPath};
use serde::Deserialize;

use crate::codec;
use crate::error::Result;
use crate::types::{LoopRegion, SoundKind, SoundSource};

/// Folders searched for `bgm/index.ron`, in order; the first that has one is the audio root.
pub const AUDIO_ROOTS: [&str; 2] = ["assets/audio", "audio"];

/// One BGM track.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BgmTrack {
    /// File under `<root>/bgm/` (default `<id>.ogg`).
    pub file: Option<String>,
    /// Optional file under `<root>/bgm/` played once before the loop starts.
    pub intro: Option<String>,
    /// Loop start in samples (default `0`).
    pub loop_start: u64,
    /// Loop end in samples (default: the end of the file).
    pub loop_end: Option<u64>,
    /// Track volume, `0.0..=1.0` (default `1.0`).
    pub volume: f32,
}

impl Default for BgmTrack {
    fn default() -> Self {
        Self {
            file: None,
            intro: None,
            loop_start: 0,
            loop_end: None,
            volume: 1.0,
        }
    }
}

/// What a jingle does to the BGM while it plays (§11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum JingleMode {
    /// Lower the BGM, then raise it again (item get).
    #[default]
    Duck,
    /// Pause the BGM, then resume it where it stopped (level up, victory).
    Pause,
}

/// One jingle.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JingleEntry {
    /// File under `<root>/bgm/` (default `<id>.ogg`).
    pub file: Option<String>,
    /// Duck or pause the BGM.
    pub mode: JingleMode,
    /// Jingle volume (default `1.0`).
    pub volume: f32,
}

impl Default for JingleEntry {
    fn default() -> Self {
        Self {
            file: None,
            mode: JingleMode::Duck,
            volume: 1.0,
        }
    }
}

/// One sound effect.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SfxEntry {
    /// File under `<root>/sfx/` (default `<id>.ogg`, or `<id>.wav`).
    pub file: Option<String>,
    /// Simultaneous instances allowed (default from the facade config).
    pub max_instances: Option<u32>,
    /// Effect volume (default `1.0`).
    pub volume: f32,
}

impl Default for SfxEntry {
    fn default() -> Self {
        Self {
            file: None,
            max_instances: None,
            volume: 1.0,
        }
    }
}

/// Battle music (§11): which BGM a battle plays and the victory jingle.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct BattleMusic {
    /// BGM id of wild battles.
    pub wild: Option<String>,
    /// BGM id of trainer battles (default: [`wild`](Self::wild)).
    pub trainer: Option<String>,
    /// Jingle id played when the player wins.
    pub victory: Option<String>,
    /// Jingle id for a won trainer battle (default: [`victory`](Self::victory)).
    pub trainer_victory: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ManifestFile {
    bgm: BTreeMap<String, BgmTrack>,
    jingles: BTreeMap<String, JingleEntry>,
    sfx: BTreeMap<String, SfxEntry>,
    battle: BattleMusic,
}

/// A resolved BGM: optional intro, the looping main part and its volume.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedBgm {
    /// Played once first.
    pub intro: Option<SoundSource>,
    /// Loops.
    pub main: SoundSource,
    /// Track volume.
    pub volume: f32,
}

/// The parsed audio manifest plus the folder it lives in.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioManifest {
    root: String,
    /// BGM tracks by id.
    pub bgm: BTreeMap<String, BgmTrack>,
    /// Jingles by id.
    pub jingles: BTreeMap<String, JingleEntry>,
    /// Sound effects by id.
    pub sfx: BTreeMap<String, SfxEntry>,
    /// Battle music.
    pub battle: BattleMusic,
    /// SFX files found in `<root>/sfx/`, by stem (to pick `.wav` when there is no `.ogg`).
    sfx_files: BTreeMap<String, String>,
}

impl Default for AudioManifest {
    fn default() -> Self {
        Self::empty(AUDIO_ROOTS[1])
    }
}

impl AudioManifest {
    /// A manifest without entries rooted at `root` (`audio`).
    #[must_use]
    pub fn empty(root: &str) -> Self {
        Self {
            root: root.trim_end_matches('/').to_owned(),
            bgm: BTreeMap::new(),
            jingles: BTreeMap::new(),
            sfx: BTreeMap::new(),
            battle: BattleMusic::default(),
            sfx_files: BTreeMap::new(),
        }
    }

    /// Parses manifest `source` (named `file` in errors) rooted at `root`.
    pub fn parse(source: &str, file: &str, root: &str) -> Result<Self> {
        let parsed: ManifestFile = codec::decode(source, file)?;
        Ok(Self {
            bgm: parsed.bgm,
            jingles: parsed.jingles,
            sfx: parsed.sfx,
            battle: parsed.battle,
            ..Self::empty(root)
        })
    }

    /// Loads the manifest of a project: the first of [`AUDIO_ROOTS`] holding `bgm/index.ron`.
    /// A project without one gets an empty manifest rooted at `audio` (defaults for every
    /// sound). A malformed manifest is an error.
    pub fn load(vfs: &dyn Vfs) -> Result<Self> {
        let mut manifest = None;
        for root in AUDIO_ROOTS {
            let path = VfsPath::new(&format!("{root}/bgm/index.ron"))?;
            if vfs.exists(&path) {
                let bytes = vfs.read(&path)?;
                let source = String::from_utf8_lossy(&bytes);
                manifest = Some(Self::parse(&source, path.as_str(), root)?);
                break;
            }
        }
        let mut manifest = match manifest {
            Some(m) => m,
            None => {
                let root = AUDIO_ROOTS
                    .into_iter()
                    .find(|root| VfsPath::new(root).is_ok_and(|p| vfs.list(&p).is_ok()))
                    .unwrap_or(AUDIO_ROOTS[1]);
                Self::empty(root)
            }
        };
        if let Ok(dir) = VfsPath::new(&format!("{}/sfx", manifest.root)) {
            for entry in vfs.list(&dir).unwrap_or_default() {
                if entry.kind != EntryKind::File {
                    continue;
                }
                let Some(name) = entry.path.file_name() else {
                    continue;
                };
                if let Some((stem, ext)) = name.rsplit_once('.') {
                    let preferred = ext == "ogg" || !manifest.sfx_files.contains_key(stem);
                    if matches!(ext, "ogg" | "wav") && preferred {
                        manifest.sfx_files.insert(stem.to_owned(), name.to_owned());
                    }
                }
            }
        }
        Ok(manifest)
    }

    /// The audio root folder (`audio` or `assets/audio`).
    #[must_use]
    pub fn root(&self) -> &str {
        &self.root
    }

    /// BGM `id` resolved to files and loop points.
    #[must_use]
    pub fn resolve_bgm(&self, id: &str) -> ResolvedBgm {
        let track = self.bgm.get(id).cloned().unwrap_or_default();
        let file = track.file.unwrap_or_else(|| format!("{id}.ogg"));
        ResolvedBgm {
            intro: track.intro.map(|intro| SoundSource {
                id: format!("{id}:intro"),
                path: format!("{}/bgm/{intro}", self.root),
                kind: SoundKind::Stream,
                looping: None,
            }),
            main: SoundSource {
                id: id.to_owned(),
                path: format!("{}/bgm/{file}", self.root),
                kind: SoundKind::Stream,
                looping: Some(LoopRegion {
                    start: track.loop_start,
                    end: track.loop_end,
                }),
            },
            volume: clamp_volume(track.volume),
        }
    }

    /// Jingle `id`: its source, mode and volume.
    #[must_use]
    pub fn resolve_jingle(&self, id: &str) -> (SoundSource, JingleMode, f32) {
        let entry = self.jingles.get(id).cloned().unwrap_or_default();
        let file = entry.file.unwrap_or_else(|| format!("{id}.ogg"));
        (
            SoundSource {
                id: id.to_owned(),
                path: format!("{}/bgm/{file}", self.root),
                kind: SoundKind::Stream,
                looping: None,
            },
            entry.mode,
            clamp_volume(entry.volume),
        )
    }

    /// SFX `id`: its source, instance limit (`default_max` unless set) and volume.
    #[must_use]
    pub fn resolve_sfx(&self, id: &str, default_max: u32) -> (SoundSource, u32, f32) {
        let entry = self.sfx.get(id).cloned().unwrap_or_default();
        let file = entry
            .file
            .or_else(|| self.sfx_files.get(id).cloned())
            .unwrap_or_else(|| format!("{id}.ogg"));
        (
            SoundSource {
                id: id.to_owned(),
                path: format!("{}/sfx/{file}", self.root),
                kind: SoundKind::Static,
                looping: None,
            },
            entry.max_instances.unwrap_or(default_max).max(1),
            clamp_volume(entry.volume),
        )
    }
}

/// `v` clamped to `0.0..=1.0` (`NaN` becomes `0.0`).
pub(crate) fn clamp_volume(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkrs_assets::MemoryVfs;

    const MANIFEST: &str = r#"(
        bgm: {
            "town": (file: "town_theme.ogg", loop_start: 88200, loop_end: 2116800),
            "battle_wild": (intro: "battle_wild_intro.ogg", volume: 0.8),
        },
        jingles: { "victory": (mode: Pause) },
        sfx: { "bump": (max_instances: 1) },
        battle: (wild: "battle_wild", victory: "victory"),
    )"#;

    #[test]
    fn parses_loop_points_intros_and_battle_music() {
        let m = AudioManifest::parse(MANIFEST, "audio/bgm/index.ron", "audio").unwrap();
        let town = m.resolve_bgm("town");
        assert_eq!(town.main.path, "audio/bgm/town_theme.ogg");
        assert_eq!(
            town.main.looping,
            Some(LoopRegion {
                start: 88200,
                end: Some(2_116_800)
            })
        );
        assert!(town.intro.is_none());
        let battle = m.resolve_bgm("battle_wild");
        assert_eq!(
            battle.intro.map(|s| s.path),
            Some("audio/bgm/battle_wild_intro.ogg".to_owned())
        );
        assert!((battle.volume - 0.8).abs() < f32::EPSILON);
        assert_eq!(m.battle.wild.as_deref(), Some("battle_wild"));
        assert_eq!(m.resolve_jingle("victory").1, JingleMode::Pause);
        assert_eq!(m.resolve_jingle("item_get").1, JingleMode::Duck);
        assert_eq!(m.resolve_sfx("bump", 4).1, 1);
        assert_eq!(m.resolve_sfx("door", 4).1, 4);
    }

    #[test]
    fn unknown_fields_are_reported_with_the_file() {
        let err = AudioManifest::parse(
            "(bgm: { \"a\": (loop: 1) })",
            "audio/bgm/index.ron",
            "audio",
        )
        .unwrap_err();
        assert!(err.to_string().starts_with("audio/bgm/index.ron:"), "{err}");
    }

    #[test]
    fn load_finds_the_root_and_wav_effects() {
        let vfs = MemoryVfs::new()
            .with("assets/audio/bgm/index.ron", "(bgm: {})")
            .unwrap()
            .with("assets/audio/sfx/door.wav", "")
            .unwrap()
            .with("assets/audio/sfx/bump.wav", "")
            .unwrap()
            .with("assets/audio/sfx/bump.ogg", "")
            .unwrap();
        let m = AudioManifest::load(&vfs).unwrap();
        assert_eq!(m.root(), "assets/audio");
        assert_eq!(m.resolve_sfx("door", 4).0.path, "assets/audio/sfx/door.wav");
        assert_eq!(m.resolve_sfx("bump", 4).0.path, "assets/audio/sfx/bump.ogg");
        assert_eq!(
            m.resolve_bgm("route1").main.path,
            "assets/audio/bgm/route1.ogg"
        );

        let empty = AudioManifest::load(&MemoryVfs::new()).unwrap();
        assert_eq!(empty.root(), "audio");
    }
}
