//! The `AudioBackend` port and its adapters (§4.5).
//!
//! Each vendor adapter lives in its own module behind a cargo feature and is the only place
//! that vendor's types appear.

#[cfg(feature = "kira")]
mod kira;
mod null;

#[cfg(feature = "kira")]
pub use self::kira::KiraAudio;
pub use self::null::{AudioLog, DEFAULT_NULL_SOUND_TICKS, LoggedCommand, NullAudio};

use crate::Result;
use crate::types::{BackendCommand, VoiceId};

/// Engine-owned port over an audio output (§4.5, §11).
///
/// It only plays, pauses, fades and stops voices and sets group volumes; every policy
/// (crossfades, ducking, battle transitions, instance limits) lives in the
/// [`Audio`](crate::Audio) facade, which calls the port once per tick when it flushes its
/// command queue (§3.3 step 6).
pub trait AudioBackend {
    /// A short adapter name for logs (`kira`, `null`).
    fn name(&self) -> &'static str;

    /// Applies one command. A failure (missing file, undecodable data, full mixer) is
    /// reported and leaves the backend usable; the voice then simply does not play.
    fn execute(&mut self, command: &BackendCommand) -> Result<()>;

    /// `true` while `voice` is playing or paused, `false` once it ended or was stopped.
    fn is_playing(&self, voice: VoiceId) -> bool;

    /// Called once per flush, after the tick's commands (simulated time advances by one
    /// tick for [`NullAudio`]).
    fn end_tick(&mut self) {}
}
