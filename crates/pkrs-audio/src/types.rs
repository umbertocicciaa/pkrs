//! Engine-owned audio vocabulary shared by the facade, the port and the adapters (§4.5).

use core::fmt;

/// A volume group (§11). Every voice plays in one of [`Bgm`](Self::Bgm), [`Sfx`](Self::Sfx)
/// or [`Jingle`](Self::Jingle); [`Master`](Self::Master) scales all of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VolumeGroup {
    /// Everything.
    Master,
    /// Background music (overworld and battle tracks).
    Bgm,
    /// Sound effects.
    Sfx,
    /// Jingles (item get, level up, victory).
    Jingle,
}

impl VolumeGroup {
    /// Every group, master first.
    pub const ALL: [Self; 4] = [Self::Master, Self::Bgm, Self::Sfx, Self::Jingle];

    /// The lowercase name used by `audio.set_volume` and the logs (`master`, `bgm`, …).
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Master => "master",
            Self::Bgm => "bgm",
            Self::Sfx => "sfx",
            Self::Jingle => "jingle",
        }
    }

    /// Parses [`name`](Self::name).
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.name() == name)
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Self::Master => 0,
            Self::Bgm => 1,
            Self::Sfx => 2,
            Self::Jingle => 3,
        }
    }
}

impl fmt::Display for VolumeGroup {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// One playing instance of a sound, allocated by the [`Audio`](crate::Audio) facade. Ids are
/// never reused within a session.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VoiceId(pub u64);

impl fmt::Display for VoiceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// Loop points of a looping sound, in samples (per channel) of the decoded file (§11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoopRegion {
    /// Where playback jumps back to (`0` = the start of the file).
    pub start: u64,
    /// Where playback jumps from (`None` = the end of the file).
    pub end: Option<u64>,
}

/// How a sound is decoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoundKind {
    /// Decoded while playing (BGM and jingles: long OGG files).
    Stream,
    /// Decoded once and kept in memory (SFX: short and played often).
    Static,
}

/// Where a sound comes from and how it plays. Paths are relative to the project VFS (§13).
#[derive(Clone, Debug, PartialEq)]
pub struct SoundSource {
    /// Sound id (BGM id, jingle id, SFX id) used in logs.
    pub id: String,
    /// VFS path of the encoded file (`audio/bgm/town.ogg`).
    pub path: String,
    /// Streamed or static.
    pub kind: SoundKind,
    /// `Some` for a looping sound.
    pub looping: Option<LoopRegion>,
}

/// One instruction for an [`AudioBackend`](crate::AudioBackend), produced by the facade's
/// per-tick flush (§3.3 step 6). Volumes are linear amplitudes in `0.0..=1.0`; fades are in
/// simulation ticks (60 Hz).
#[derive(Clone, Debug, PartialEq)]
pub enum BackendCommand {
    /// Starts `source` as `voice` in `group`, fading in from silence over `fade_in` ticks.
    Play {
        /// New voice.
        voice: VoiceId,
        /// Volume group it plays in.
        group: VolumeGroup,
        /// The sound.
        source: SoundSource,
        /// Voice volume.
        volume: f32,
        /// Fade-in length (`0` = start at `volume`).
        fade_in: u32,
    },
    /// Changes a voice's volume.
    SetVolume {
        /// Voice.
        voice: VoiceId,
        /// New volume.
        volume: f32,
        /// Fade length.
        fade: u32,
    },
    /// Pauses a voice, keeping its position.
    Pause {
        /// Voice.
        voice: VoiceId,
        /// Fade-out length.
        fade: u32,
    },
    /// Resumes a paused voice where it stopped.
    Resume {
        /// Voice.
        voice: VoiceId,
        /// Fade-in length.
        fade: u32,
    },
    /// Stops a voice for good.
    Stop {
        /// Voice.
        voice: VoiceId,
        /// Fade-out length.
        fade: u32,
    },
    /// Changes the volume of a whole group.
    SetGroupVolume {
        /// Group.
        group: VolumeGroup,
        /// New volume.
        volume: f32,
        /// Fade length.
        fade: u32,
    },
}

impl BackendCommand {
    /// The voice this command addresses, if any.
    #[must_use]
    pub fn voice(&self) -> Option<VoiceId> {
        match self {
            Self::Play { voice, .. }
            | Self::SetVolume { voice, .. }
            | Self::Pause { voice, .. }
            | Self::Resume { voice, .. }
            | Self::Stop { voice, .. } => Some(*voice),
            Self::SetGroupVolume { .. } => None,
        }
    }
}

/// Compact form for logs and test assertions: `play #1 bgm:town fade=0`, `stop #1 fade=30`,
/// `volume #1 0.30 fade=6`, `group master 0.25 fade=0`, …
impl fmt::Display for BackendCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Play {
                voice,
                group,
                source,
                volume,
                fade_in,
            } => {
                write!(f, "play {voice} {group}:{} fade={fade_in}", source.id)?;
                if (*volume - 1.0).abs() > f32::EPSILON {
                    write!(f, " vol={volume:.2}")?;
                }
                Ok(())
            }
            Self::SetVolume {
                voice,
                volume,
                fade,
            } => write!(f, "volume {voice} {volume:.2} fade={fade}"),
            Self::Pause { voice, fade } => write!(f, "pause {voice} fade={fade}"),
            Self::Resume { voice, fade } => write!(f, "resume {voice} fade={fade}"),
            Self::Stop { voice, fade } => write!(f, "stop {voice} fade={fade}"),
            Self::SetGroupVolume {
                group,
                volume,
                fade,
            } => write!(f, "group {group} {volume:.2} fade={fade}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_names_round_trip() {
        for group in VolumeGroup::ALL {
            assert_eq!(VolumeGroup::parse(group.name()), Some(group));
        }
        assert_eq!(VolumeGroup::parse("music"), None);
    }

    #[test]
    fn commands_display_compactly() {
        let play = BackendCommand::Play {
            voice: VoiceId(3),
            group: VolumeGroup::Bgm,
            source: SoundSource {
                id: "town".into(),
                path: "audio/bgm/town.ogg".into(),
                kind: SoundKind::Stream,
                looping: Some(LoopRegion::default()),
            },
            volume: 1.0,
            fade_in: 30,
        };
        assert_eq!(play.to_string(), "play #3 bgm:town fade=30");
        assert_eq!(play.voice(), Some(VoiceId(3)));
        let duck = BackendCommand::SetVolume {
            voice: VoiceId(3),
            volume: 0.3,
            fade: 6,
        };
        assert_eq!(duck.to_string(), "volume #3 0.30 fade=6");
        let group = BackendCommand::SetGroupVolume {
            group: VolumeGroup::Master,
            volume: 0.25,
            fade: 0,
        };
        assert_eq!(group.to_string(), "group master 0.25 fade=0");
        assert_eq!(group.voice(), None);
    }
}
