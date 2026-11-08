//! `KiraAudio`: the [`AudioBackend`] adapter over `kira` (§4.5, §11). The only module that
//! uses `kira` types.
//!
//! - One kira sub-track per [`VolumeGroup`] (bgm, sfx, jingle) under the main track (master).
//! - BGM and jingles stream from the encoded bytes read through the project [`Vfs`] (read
//!   once and kept); SFX are decoded once into static sounds.
//! - Loop points are sample positions ([`LoopRegion`]); fades are ticks turned into kira
//!   tweens at 60 Hz.
//! - Opening the output can fail (no sound device on CI runners, headless servers):
//!   [`KiraAudio::open_or_null`] then logs and falls back to [`NullAudio`].

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

use kira::backend::Backend;
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::streaming::{StreamingSoundData, StreamingSoundHandle};
use kira::sound::{EndPosition, FromFileError, PlaybackPosition, PlaybackState, Region};
use kira::track::{TrackBuilder, TrackHandle};
use kira::{AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Tween};
use pkrs_assets::{Vfs, VfsPath};

use super::{AudioBackend, NullAudio};
use crate::error::{AudioError, Result};
use crate::types::{BackendCommand, LoopRegion, SoundKind, SoundSource, VoiceId, VolumeGroup};

/// Simulation ticks per second (§3.1), for turning fades into durations.
const TICKS_PER_SECOND: f64 = 60.0;

/// The kira-backed [`AudioBackend`]: real sound output through cpal.
pub struct KiraAudio {
    core: Core<DefaultBackend>,
}

impl KiraAudio {
    /// Opens the default output device; sounds are read from `vfs`.
    ///
    /// Errors with [`AudioError::NoDevice`] when no output device can be opened.
    pub fn new(vfs: Box<dyn Vfs>) -> Result<Self> {
        Ok(Self {
            core: Core::new(AudioManagerSettings::default(), vfs)?,
        })
    }

    /// [`KiraAudio`] on the default device, or [`NullAudio`] (logged) when there is none, so
    /// a machine without sound still runs the game.
    #[must_use]
    pub fn open_or_null(vfs: Box<dyn Vfs>) -> Box<dyn AudioBackend> {
        match Self::new(vfs) {
            Ok(kira) => Box::new(kira),
            Err(err) => {
                tracing::warn!(target: "audio", "{err}; continuing without sound (NullAudio)");
                Box::new(NullAudio::new())
            }
        }
    }
}

impl AudioBackend for KiraAudio {
    fn name(&self) -> &'static str {
        "kira"
    }

    fn execute(&mut self, command: &BackendCommand) -> Result<()> {
        self.core.execute(command)
    }

    fn is_playing(&self, voice: VoiceId) -> bool {
        self.core.is_playing(voice)
    }

    fn end_tick(&mut self) {
        self.core.prune();
    }
}

enum Voice {
    Static(StaticSoundHandle),
    Stream(StreamingSoundHandle<FromFileError>),
}

impl Voice {
    fn state(&self) -> PlaybackState {
        match self {
            Self::Static(h) => h.state(),
            Self::Stream(h) => h.state(),
        }
    }

    fn set_volume(&mut self, volume: Decibels, tween: Tween) {
        match self {
            Self::Static(h) => h.set_volume(volume, tween),
            Self::Stream(h) => h.set_volume(volume, tween),
        }
    }

    fn pause(&mut self, tween: Tween) {
        match self {
            Self::Static(h) => h.pause(tween),
            Self::Stream(h) => h.pause(tween),
        }
    }

    fn resume(&mut self, tween: Tween) {
        match self {
            Self::Static(h) => h.resume(tween),
            Self::Stream(h) => h.resume(tween),
        }
    }

    fn stop(&mut self, tween: Tween) {
        match self {
            Self::Static(h) => h.stop(tween),
            Self::Stream(h) => h.stop(tween),
        }
    }
}

/// The adapter logic, generic over the kira backend so tests run on kira's mock backend.
struct Core<B: Backend> {
    manager: AudioManager<B>,
    /// Sub-tracks for bgm, sfx and jingle.
    tracks: [TrackHandle; 3],
    vfs: Box<dyn Vfs>,
    encoded: BTreeMap<String, Arc<[u8]>>,
    statics: BTreeMap<String, StaticSoundData>,
    voices: BTreeMap<VoiceId, Voice>,
}

impl<B: Backend> Core<B>
where
    B::Error: std::fmt::Debug,
{
    fn new(settings: AudioManagerSettings<B>, vfs: Box<dyn Vfs>) -> Result<Self> {
        let mut manager =
            AudioManager::<B>::new(settings).map_err(|e| AudioError::NoDevice(format!("{e:?}")))?;
        let mut track = || {
            manager
                .add_sub_track(TrackBuilder::new())
                .map_err(|e| AudioError::Backend(format!("cannot create a mixer track: {e}")))
        };
        let tracks = [track()?, track()?, track()?];
        Ok(Self {
            manager,
            tracks,
            vfs,
            encoded: BTreeMap::new(),
            statics: BTreeMap::new(),
            voices: BTreeMap::new(),
        })
    }
}

impl<B: Backend> Core<B> {
    fn execute(&mut self, command: &BackendCommand) -> Result<()> {
        match command {
            BackendCommand::Play {
                voice,
                group,
                source,
                volume,
                fade_in,
            } => {
                let handle = self.play(*group, source, *volume, *fade_in)?;
                self.voices.insert(*voice, handle);
            }
            BackendCommand::SetVolume {
                voice,
                volume,
                fade,
            } => {
                if let Some(v) = self.voices.get_mut(voice) {
                    v.set_volume(decibels(*volume), tween(*fade));
                }
            }
            BackendCommand::Pause { voice, fade } => {
                if let Some(v) = self.voices.get_mut(voice) {
                    v.pause(tween(*fade));
                }
            }
            BackendCommand::Resume { voice, fade } => {
                if let Some(v) = self.voices.get_mut(voice) {
                    v.resume(tween(*fade));
                }
            }
            BackendCommand::Stop { voice, fade } => {
                if let Some(mut v) = self.voices.remove(voice) {
                    v.stop(tween(*fade));
                }
            }
            BackendCommand::SetGroupVolume {
                group,
                volume,
                fade,
            } => {
                let (db, tween) = (decibels(*volume), tween(*fade));
                match group {
                    VolumeGroup::Master => self.manager.main_track().set_volume(db, tween),
                    other => {
                        if let Some(track) = self.track(*other) {
                            track.set_volume(db, tween);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn track(&mut self, group: VolumeGroup) -> Option<&mut TrackHandle> {
        let index = match group {
            VolumeGroup::Master => return None,
            VolumeGroup::Bgm => 0,
            VolumeGroup::Sfx => 1,
            VolumeGroup::Jingle => 2,
        };
        self.tracks.get_mut(index)
    }

    fn encoded(&mut self, path: &str) -> Result<Arc<[u8]>> {
        if let Some(bytes) = self.encoded.get(path) {
            return Ok(bytes.clone());
        }
        let bytes: Arc<[u8]> = self.vfs.read(&VfsPath::new(path)?)?.into();
        self.encoded.insert(path.to_owned(), bytes.clone());
        Ok(bytes)
    }

    fn play(
        &mut self,
        group: VolumeGroup,
        source: &SoundSource,
        volume: f32,
        fade_in: u32,
    ) -> Result<Voice> {
        let fade = (fade_in > 0).then(|| tween(fade_in));
        let region = source.looping.map(loop_region);
        let decode_error = |message: String| AudioError::Decode {
            path: source.path.clone(),
            message,
        };
        match source.kind {
            SoundKind::Stream => {
                let bytes = self.encoded(&source.path)?;
                let data = StreamingSoundData::from_cursor(Cursor::new(bytes))
                    .map_err(|e| decode_error(e.to_string()))?
                    .loop_region(region)
                    .volume(decibels(volume))
                    .fade_in_tween(fade);
                let track = self
                    .track(group)
                    .ok_or_else(|| AudioError::Backend("no track for master".into()))?;
                track
                    .play(data)
                    .map(Voice::Stream)
                    .map_err(|e| AudioError::Backend(e.to_string()))
            }
            SoundKind::Static => {
                let data = match self.statics.get(&source.path) {
                    Some(data) => data.clone(),
                    None => {
                        let bytes = self.encoded(&source.path)?;
                        let data = StaticSoundData::from_cursor(Cursor::new(bytes))
                            .map_err(|e| decode_error(e.to_string()))?;
                        self.statics.insert(source.path.clone(), data.clone());
                        data
                    }
                };
                let data = data
                    .loop_region(region)
                    .volume(decibels(volume))
                    .fade_in_tween(fade);
                let track = self
                    .track(group)
                    .ok_or_else(|| AudioError::Backend("no track for master".into()))?;
                track
                    .play(data)
                    .map(Voice::Static)
                    .map_err(|e| AudioError::Backend(e.to_string()))
            }
        }
    }

    fn is_playing(&self, voice: VoiceId) -> bool {
        self.voices
            .get(&voice)
            .is_some_and(|v| v.state() != PlaybackState::Stopped)
    }

    /// Forgets voices that ended on their own.
    fn prune(&mut self) {
        self.voices
            .retain(|_, v| v.state() != PlaybackState::Stopped);
    }
}

fn loop_region(region: LoopRegion) -> Region {
    let to_samples = |s: u64| usize::try_from(s).unwrap_or(usize::MAX);
    Region {
        start: PlaybackPosition::Samples(to_samples(region.start)),
        end: match region.end {
            Some(end) => EndPosition::Custom(PlaybackPosition::Samples(to_samples(end))),
            None => EndPosition::EndOfAudio,
        },
    }
}

/// Linear amplitude to kira decibels (silence below -60 dB).
fn decibels(amplitude: f32) -> Decibels {
    if amplitude.is_nan() || amplitude <= 0.001 {
        Decibels::SILENCE
    } else {
        Decibels(20.0 * amplitude.min(1.0).log10())
    }
}

fn tween(ticks: u32) -> Tween {
    Tween {
        duration: Duration::from_secs_f64(f64::from(ticks) / TICKS_PER_SECOND)
            .max(Duration::from_millis(1)),
        ..Tween::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::backend::mock::{MockBackend, MockBackendSettings};
    use pkrs_assets::MemoryVfs;

    /// A tiny mono 16-bit PCM WAV of `frames` silent frames.
    fn wav(frames: u32) -> Vec<u8> {
        let data_len = frames * 2;
        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + data_len).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&8000u32.to_le_bytes());
        out.extend_from_slice(&16000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        out.resize(out.len() + data_len as usize, 0);
        out
    }

    fn core() -> Core<MockBackend> {
        let vfs = MemoryVfs::new()
            .with("audio/sfx/blip.wav", wav(800))
            .unwrap()
            .with("audio/bgm/loop.wav", wav(800))
            .unwrap();
        let settings = AudioManagerSettings {
            backend_settings: MockBackendSettings { sample_rate: 8000 },
            ..AudioManagerSettings::default()
        };
        Core::new(settings, Box::new(vfs)).unwrap()
    }

    fn source(id: &str, path: &str, kind: SoundKind, looping: bool) -> SoundSource {
        SoundSource {
            id: id.into(),
            path: path.into(),
            kind,
            looping: looping.then_some(LoopRegion {
                start: 100,
                end: Some(700),
            }),
        }
    }

    #[test]
    fn plays_pauses_and_stops_voices_on_the_mock_backend() {
        let mut core = core();
        let play = |voice, kind, path: &str, looping| BackendCommand::Play {
            voice: VoiceId(voice),
            group: VolumeGroup::Bgm,
            source: source("s", path, kind, looping),
            volume: 0.5,
            fade_in: 30,
        };
        core.execute(&play(1, SoundKind::Stream, "audio/bgm/loop.wav", true))
            .unwrap();
        core.execute(&play(2, SoundKind::Static, "audio/sfx/blip.wav", false))
            .unwrap();
        assert!(core.is_playing(VoiceId(1)) && core.is_playing(VoiceId(2)));
        for command in [
            BackendCommand::SetVolume {
                voice: VoiceId(1),
                volume: 0.3,
                fade: 6,
            },
            BackendCommand::Pause {
                voice: VoiceId(1),
                fade: 0,
            },
            BackendCommand::Resume {
                voice: VoiceId(1),
                fade: 15,
            },
            BackendCommand::SetGroupVolume {
                group: VolumeGroup::Master,
                volume: 0.25,
                fade: 6,
            },
            BackendCommand::SetGroupVolume {
                group: VolumeGroup::Sfx,
                volume: 0.0,
                fade: 0,
            },
            BackendCommand::Stop {
                voice: VoiceId(2),
                fade: 0,
            },
        ] {
            core.execute(&command).unwrap();
        }
        assert!(!core.is_playing(VoiceId(2)));
        assert!(core.is_playing(VoiceId(1)));
        core.prune();
        assert_eq!(core.encoded.len(), 2, "encoded bytes are cached");
        assert_eq!(core.statics.len(), 1, "static sounds decode once");
    }

    #[test]
    fn missing_and_undecodable_files_are_errors_not_panics() {
        let mut core = core();
        let missing = BackendCommand::Play {
            voice: VoiceId(1),
            group: VolumeGroup::Sfx,
            source: source("nope", "audio/sfx/nope.ogg", SoundKind::Static, false),
            volume: 1.0,
            fade_in: 0,
        };
        assert!(matches!(core.execute(&missing), Err(AudioError::Assets(_))));
        let mut vfs_core = core;
        vfs_core
            .encoded
            .insert("audio/bgm/bad.ogg".into(), Arc::from(&b"not audio"[..]));
        let bad = BackendCommand::Play {
            voice: VoiceId(2),
            group: VolumeGroup::Bgm,
            source: source("bad", "audio/bgm/bad.ogg", SoundKind::Stream, true),
            volume: 1.0,
            fade_in: 0,
        };
        assert!(matches!(
            vfs_core.execute(&bad),
            Err(AudioError::Decode { .. })
        ));
        assert!(!vfs_core.is_playing(VoiceId(2)));
    }

    #[test]
    fn volume_and_fade_conversions() {
        assert_eq!(decibels(0.0), Decibels::SILENCE);
        assert_eq!(decibels(f32::NAN), Decibels::SILENCE);
        assert!((decibels(1.0).0).abs() < 1e-6);
        assert!((decibels(0.5).0 + 6.0206).abs() < 1e-3);
        assert_eq!(tween(30).duration, Duration::from_millis(500));
        assert_eq!(tween(0).duration, Duration::from_millis(1));
        let r = loop_region(LoopRegion {
            start: 5,
            end: None,
        });
        assert_eq!(r.start, PlaybackPosition::Samples(5));
        assert_eq!(r.end, EndPosition::EndOfAudio);
    }
}
