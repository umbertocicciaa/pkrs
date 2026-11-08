//! Audio facade and `AudioBackend` port: BGM, SFX, jingles, crossfades and ducking (§11).
//!
//! Layout (Ports & Adapters, `docs/design.md` §4.5):
//!
//! - **Port**: [`AudioBackend`] — plays, pauses, fades and stops voices and sets group
//!   volumes, in engine vocabulary ([`BackendCommand`], [`VoiceId`], [`VolumeGroup`]).
//! - **Facade**: [`Audio`] — a per-tick command queue with the §11 policy on top: same BGM id
//!   keeps playing, another id crossfades (30 ticks by default), battle music pauses and
//!   resumes the overworld track, jingles duck or pause the BGM, SFX are instance-limited,
//!   volume groups (master / bgm / sfx / jingle) and focus ducking.
//! - **Adapters**: [`KiraAudio`] (feature `kira`: OGG streaming with loop points, cpal output)
//!   and the vendor-free [`NullAudio`], which records commands for tests and headless runs.
//! - **Manifest**: [`AudioManifest`] reads `audio/bgm/index.ron` (loop points, intros,
//!   jingle modes, SFX limits, battle music).
//!
//! No vendor type appears in any public signature.

mod audio;
mod backend;
mod codec;
mod error;
mod manifest;
mod types;

pub use audio::{
    Audio, AudioConfig, AudioRequest, BattleKind, BattleResume, DEFAULT_CROSSFADE_TICKS,
};
#[cfg(feature = "kira")]
pub use backend::KiraAudio;
pub use backend::{AudioBackend, AudioLog, DEFAULT_NULL_SOUND_TICKS, LoggedCommand, NullAudio};
pub use error::{AudioError, Result};
pub use manifest::{
    AUDIO_ROOTS, AudioManifest, BattleMusic, BgmTrack, JingleEntry, JingleMode, ResolvedBgm,
    SfxEntry,
};
pub use types::{BackendCommand, LoopRegion, SoundKind, SoundSource, VoiceId, VolumeGroup};

#[cfg(test)]
mod tests;
