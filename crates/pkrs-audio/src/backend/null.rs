//! `NullAudio`: a silent backend that records every command, for tests and `--headless`
//! (§4.5, §15.2).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use super::AudioBackend;
use crate::Result;
use crate::types::{BackendCommand, VoiceId};

/// How long a non-looping sound "plays" on [`NullAudio`] unless set with
/// [`NullAudio::with_length`]: one second at 60 Hz.
pub const DEFAULT_NULL_SOUND_TICKS: u32 = 60;

/// One command received by a [`NullAudio`].
#[derive(Clone, Debug, PartialEq)]
pub struct LoggedCommand {
    /// Flushes completed before this command (the facade flushes once per tick, so this is
    /// the tick index counted from the first flush).
    pub tick: u64,
    /// The command.
    pub command: BackendCommand,
}

/// Shared, cloneable view of the commands a [`NullAudio`] received.
#[derive(Clone, Debug, Default)]
pub struct AudioLog(Rc<RefCell<Vec<LoggedCommand>>>);

impl AudioLog {
    /// Every command so far, oldest first.
    #[must_use]
    pub fn entries(&self) -> Vec<LoggedCommand> {
        self.0.borrow().clone()
    }

    /// Every command so far in its compact [`Display`](std::fmt::Display) form.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.0
            .borrow()
            .iter()
            .map(|e| e.command.to_string())
            .collect()
    }

    /// Number of commands recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.borrow().len()
    }

    /// `true` when nothing was recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.borrow().is_empty()
    }

    /// Forgets every recorded command.
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    fn push(&self, entry: LoggedCommand) {
        self.0.borrow_mut().push(entry);
    }
}

#[derive(Clone, Debug)]
struct NullVoice {
    /// Ticks left before a non-looping voice ends; `None` loops forever.
    remaining: Option<u32>,
    paused: bool,
}

/// Silent [`AudioBackend`]: records commands in an [`AudioLog`] and simulates voice
/// lifetimes in ticks, so the facade's timed policy (intro → loop, jingle end) is testable
/// without a sound device.
#[derive(Debug)]
pub struct NullAudio {
    log: AudioLog,
    voices: BTreeMap<VoiceId, NullVoice>,
    lengths: BTreeMap<String, u32>,
    default_length: u32,
    tick: u64,
}

impl Default for NullAudio {
    fn default() -> Self {
        Self::new()
    }
}

impl NullAudio {
    /// A backend where every non-looping sound lasts [`DEFAULT_NULL_SOUND_TICKS`].
    #[must_use]
    pub fn new() -> Self {
        Self {
            log: AudioLog::default(),
            voices: BTreeMap::new(),
            lengths: BTreeMap::new(),
            default_length: DEFAULT_NULL_SOUND_TICKS,
            tick: 0,
        }
    }

    /// Non-looping sound `id` lasts `ticks` ticks (at least one).
    #[must_use]
    pub fn with_length(mut self, id: &str, ticks: u32) -> Self {
        self.lengths.insert(id.to_owned(), ticks.max(1));
        self
    }

    /// The command log (shared: keep it, then move the backend into the facade).
    #[must_use]
    pub fn log(&self) -> AudioLog {
        self.log.clone()
    }
}

impl AudioBackend for NullAudio {
    fn name(&self) -> &'static str {
        "null"
    }

    fn execute(&mut self, command: &BackendCommand) -> Result<()> {
        self.log.push(LoggedCommand {
            tick: self.tick,
            command: command.clone(),
        });
        match command {
            BackendCommand::Play { voice, source, .. } => {
                let remaining = match source.looping {
                    Some(_) => None,
                    None => Some(
                        self.lengths
                            .get(&source.id)
                            .copied()
                            .unwrap_or(self.default_length),
                    ),
                };
                self.voices.insert(
                    *voice,
                    NullVoice {
                        remaining,
                        paused: false,
                    },
                );
            }
            BackendCommand::Pause { voice, .. } => {
                if let Some(v) = self.voices.get_mut(voice) {
                    v.paused = true;
                }
            }
            BackendCommand::Resume { voice, .. } => {
                if let Some(v) = self.voices.get_mut(voice) {
                    v.paused = false;
                }
            }
            BackendCommand::Stop { voice, .. } => {
                self.voices.remove(voice);
            }
            BackendCommand::SetVolume { .. } | BackendCommand::SetGroupVolume { .. } => {}
        }
        Ok(())
    }

    fn is_playing(&self, voice: VoiceId) -> bool {
        self.voices.contains_key(&voice)
    }

    fn end_tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
        self.voices
            .retain(|_, v| match (&mut v.remaining, v.paused) {
                (Some(left), false) => {
                    *left = left.saturating_sub(1);
                    *left > 0
                }
                _ => true,
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LoopRegion, SoundKind, SoundSource, VolumeGroup};

    fn play(voice: u64, id: &str, looping: bool) -> BackendCommand {
        BackendCommand::Play {
            voice: VoiceId(voice),
            group: VolumeGroup::Sfx,
            source: SoundSource {
                id: id.into(),
                path: format!("audio/sfx/{id}.ogg"),
                kind: SoundKind::Static,
                looping: looping.then(LoopRegion::default),
            },
            volume: 1.0,
            fade_in: 0,
        }
    }

    #[test]
    fn one_shots_end_after_their_length_and_loops_never() {
        let mut null = NullAudio::new().with_length("blip", 2);
        let log = null.log();
        null.execute(&play(1, "blip", false)).unwrap();
        null.execute(&play(2, "theme", true)).unwrap();
        null.end_tick();
        assert!(null.is_playing(VoiceId(1)));
        null.end_tick();
        assert!(!null.is_playing(VoiceId(1)));
        assert!(null.is_playing(VoiceId(2)));
        assert_eq!(log.len(), 2);
        assert_eq!(log.entries()[1].tick, 0);
    }

    #[test]
    fn paused_voices_do_not_advance_and_stop_ends_them() {
        let mut null = NullAudio::new().with_length("blip", 1);
        null.execute(&play(1, "blip", false)).unwrap();
        null.execute(&BackendCommand::Pause {
            voice: VoiceId(1),
            fade: 0,
        })
        .unwrap();
        null.end_tick();
        assert!(null.is_playing(VoiceId(1)));
        null.execute(&BackendCommand::Stop {
            voice: VoiceId(1),
            fade: 0,
        })
        .unwrap();
        assert!(!null.is_playing(VoiceId(1)));
        assert_eq!(
            null.log().lines(),
            [
                "play #1 sfx:blip fade=0",
                "pause #1 fade=0",
                "stop #1 fade=0"
            ]
        );
    }
}
