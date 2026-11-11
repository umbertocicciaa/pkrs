//! The [`Audio`] facade (§4.5, §11): engine policy over an [`AudioBackend`].
//!
//! Game code and scripts only *queue* requests during the tick; [`Audio::flush`] runs once
//! per tick after the events phase (§3.3 step 6) and turns the queue, in order, into
//! [`BackendCommand`]s. Timed rules (an intro handing over to its loop, a jingle ending and
//! the BGM coming back) are checked in the same flush, so the command sequence depends only
//! on the ticks, never on wall-clock time.

use std::collections::{BTreeMap, VecDeque};

use crate::backend::{AudioBackend, AudioLog, NullAudio};
use crate::manifest::{AudioManifest, JingleMode, clamp_volume};
use crate::types::{BackendCommand, SoundSource, VoiceId, VolumeGroup};

/// Default crossfade on a BGM change, in ticks (§11: 30 ticks, half a second).
pub const DEFAULT_CROSSFADE_TICKS: u32 = 30;

/// What happens to the overworld BGM after a battle (§11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BattleResume {
    /// Resume the track from where the battle paused it.
    #[default]
    Resume,
    /// Restart the track from the beginning.
    Restart,
}

/// Which kind of battle starts (picks the battle BGM and victory jingle of the manifest).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleKind {
    /// A wild encounter.
    Wild,
    /// A trainer battle.
    Trainer,
}

/// Facade policy knobs (§11). Fades are in ticks; volumes are linear `0.0..=1.0`.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioConfig {
    /// Crossfade when the BGM id changes (map change, `audio.play_bgm` without `fade`).
    pub crossfade_ticks: u32,
    /// BGM volume factor while a [`JingleMode::Duck`] jingle plays.
    pub duck_volume: f32,
    /// Fade into and out of the jingle duck.
    pub duck_fade_ticks: u32,
    /// Master volume factor while the window is unfocused (`1.0` disables focus ducking).
    pub focus_duck_volume: f32,
    /// Fade of the battle music when a battle ends without a victory jingle.
    pub battle_fade_ticks: u32,
    /// Fade-in of the overworld BGM when it resumes after a battle or a pausing jingle.
    pub resume_fade_ticks: u32,
    /// Resume or restart the overworld BGM after a battle.
    pub battle_resume: BattleResume,
    /// Simultaneous instances of one SFX id unless the manifest says otherwise.
    pub default_sfx_instances: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            crossfade_ticks: DEFAULT_CROSSFADE_TICKS,
            duck_volume: 0.3,
            duck_fade_ticks: 6,
            focus_duck_volume: 0.25,
            battle_fade_ticks: 10,
            resume_fade_ticks: 15,
            battle_resume: BattleResume::Resume,
            default_sfx_instances: 4,
        }
    }
}

/// A request game code or a script queues (the `audio.*` Lua API, §9.1).
#[derive(Clone, Debug, PartialEq)]
pub enum AudioRequest {
    /// Play BGM `id`; same id keeps playing, another id crossfades over `fade` ticks
    /// (default [`AudioConfig::crossfade_ticks`]).
    PlayBgm {
        /// BGM id.
        id: String,
        /// Crossfade length.
        fade: Option<u32>,
    },
    /// Stop the BGM, fading out over `fade` ticks (default crossfade length).
    StopBgm {
        /// Fade-out length.
        fade: Option<u32>,
    },
    /// Play sound effect `id` (fire and forget, instance-limited).
    PlaySfx {
        /// SFX id.
        id: String,
    },
    /// Play jingle `id`, ducking or pausing the BGM until it ends.
    PlayJingle {
        /// Jingle id.
        id: String,
    },
    /// Set the user volume of `group` (`0.0..=1.0`).
    SetVolume {
        /// Group.
        group: VolumeGroup,
        /// Volume.
        volume: f32,
    },
}

#[derive(Clone, Debug)]
enum Queued {
    Request(AudioRequest),
    StartBattle(Option<String>),
    EndBattle(Option<String>),
    Focus(bool),
}

#[derive(Clone, Debug)]
struct Bgm {
    id: String,
    voice: VoiceId,
    volume: f32,
    /// The loop still to start while the intro plays.
    pending_main: Option<SoundSource>,
}

/// What a running jingle does to the BGM.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Hold {
    None,
    Ducked,
    Paused,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AfterJingle {
    Restore,
    ResumeOverworld,
}

#[derive(Clone, Debug)]
struct Jingle {
    voice: VoiceId,
    after: AfterJingle,
}

/// Audio facade: BGM with crossfades and loop points, battle music, jingles with ducking,
/// instance-limited SFX, volume groups and focus ducking (§11), over any [`AudioBackend`].
pub struct Audio {
    backend: Box<dyn AudioBackend>,
    manifest: AudioManifest,
    config: AudioConfig,
    queue: Vec<Queued>,
    next_voice: u64,
    current: Option<Bgm>,
    /// The overworld BGM, paused while a battle runs.
    suspended: Option<Bgm>,
    in_battle: bool,
    jingle: Option<Jingle>,
    hold: Hold,
    sfx: BTreeMap<String, VecDeque<VoiceId>>,
    volumes: [f32; 4],
    focused: bool,
    ticks: u64,
}

impl Audio {
    /// The facade over `backend`.
    #[must_use]
    pub fn new(
        backend: Box<dyn AudioBackend>,
        manifest: AudioManifest,
        config: AudioConfig,
    ) -> Self {
        Self {
            backend,
            manifest,
            config,
            queue: Vec::new(),
            next_voice: 1,
            current: None,
            suspended: None,
            in_battle: false,
            jingle: None,
            hold: Hold::None,
            sfx: BTreeMap::new(),
            volumes: [1.0; 4],
            focused: true,
            ticks: 0,
        }
    }

    /// The facade over a fresh [`NullAudio`], with its command log (tests, `--headless`).
    #[must_use]
    pub fn null(manifest: AudioManifest, config: AudioConfig) -> (Self, AudioLog) {
        let null = NullAudio::new();
        let log = null.log();
        (Self::new(Box::new(null), manifest, config), log)
    }

    /// Replaces the backend; queued requests stay queued. Meant for start-up (before the first
    /// [`flush`](Self::flush)): voices started on the old backend are not carried over.
    pub fn set_backend(&mut self, backend: Box<dyn AudioBackend>) {
        self.backend = backend;
    }

    /// Name of the backend in use (`kira`, `null`).
    #[must_use]
    pub fn backend_name(&self) -> &'static str {
        self.backend.name()
    }

    /// The manifest sounds are resolved with.
    #[must_use]
    pub fn manifest(&self) -> &AudioManifest {
        &self.manifest
    }

    /// The policy configuration.
    #[must_use]
    pub fn config(&self) -> &AudioConfig {
        &self.config
    }

    /// Id of the BGM currently playing (the battle track during a battle).
    #[must_use]
    pub fn current_bgm(&self) -> Option<&str> {
        self.current.as_ref().map(|b| b.id.as_str())
    }

    /// `true` between [`start_battle`](Self::start_battle) and
    /// [`end_battle`](Self::end_battle) (as of the last flush).
    #[must_use]
    pub fn in_battle(&self) -> bool {
        self.in_battle
    }

    /// `true` while a jingle plays (as of the last flush).
    #[must_use]
    pub fn jingle_playing(&self) -> bool {
        self.jingle.is_some()
    }

    /// The user volume of `group`.
    #[must_use]
    pub fn volume(&self, group: VolumeGroup) -> f32 {
        self.volumes[group.index()]
    }

    /// Flushes run so far.
    #[must_use]
    pub fn ticks(&self) -> u64 {
        self.ticks
    }

    /// Queues `request` for the next [`flush`](Self::flush).
    pub fn request(&mut self, request: AudioRequest) {
        self.queue.push(Queued::Request(request));
    }

    /// Queues [`AudioRequest::PlayBgm`].
    pub fn play_bgm(&mut self, id: &str, fade: Option<u32>) {
        self.request(AudioRequest::PlayBgm {
            id: id.to_owned(),
            fade,
        });
    }

    /// Queues [`AudioRequest::StopBgm`].
    pub fn stop_bgm(&mut self, fade: Option<u32>) {
        self.request(AudioRequest::StopBgm { fade });
    }

    /// Queues [`AudioRequest::PlaySfx`].
    pub fn play_sfx(&mut self, id: &str) {
        self.request(AudioRequest::PlaySfx { id: id.to_owned() });
    }

    /// Queues [`AudioRequest::PlayJingle`].
    pub fn play_jingle(&mut self, id: &str) {
        self.request(AudioRequest::PlayJingle { id: id.to_owned() });
    }

    /// Queues [`AudioRequest::SetVolume`].
    pub fn set_volume(&mut self, group: VolumeGroup, volume: f32) {
        self.request(AudioRequest::SetVolume { group, volume });
    }

    /// Queues the map BGM: `Some(id)` plays it (same id keeps playing, §11), `None` (a map
    /// without a `bgm` property) leaves the current track alone.
    pub fn map_bgm(&mut self, id: Option<&str>) {
        if let Some(id) = id {
            self.play_bgm(id, None);
        }
    }

    /// Queues a battle start: the overworld BGM pauses (keeping its position) and the
    /// manifest's battle track for `kind` plays, intro first (§11).
    pub fn start_battle(&mut self, kind: BattleKind) {
        let battle = &self.manifest.battle;
        let music = match kind {
            BattleKind::Wild => battle.wild.clone(),
            BattleKind::Trainer => battle.trainer.clone().or_else(|| battle.wild.clone()),
        };
        self.queue.push(Queued::StartBattle(music));
    }

    /// Queues a battle start with an explicit battle track (`None`: silence).
    pub fn start_battle_with(&mut self, music: Option<&str>) {
        self.queue
            .push(Queued::StartBattle(music.map(str::to_owned)));
    }

    /// Queues a battle end: the battle track stops, the victory jingle plays when `won`, then
    /// the overworld BGM resumes (or restarts, [`AudioConfig::battle_resume`]).
    pub fn end_battle(&mut self, kind: BattleKind, won: bool) {
        let battle = &self.manifest.battle;
        let victory = match kind {
            BattleKind::Wild => battle.victory.clone(),
            BattleKind::Trainer => battle
                .trainer_victory
                .clone()
                .or_else(|| battle.victory.clone()),
        };
        self.queue.push(Queued::EndBattle(victory.filter(|_| won)));
    }

    /// Queues a battle end with an explicit victory jingle (`None`: none).
    pub fn end_battle_with(&mut self, victory: Option<&str>) {
        self.queue
            .push(Queued::EndBattle(victory.map(str::to_owned)));
    }

    /// Queues a focus change: unfocused, the master volume ducks to
    /// [`AudioConfig::focus_duck_volume`] (§3.2).
    pub fn set_focus(&mut self, focused: bool) {
        self.queue.push(Queued::Focus(focused));
    }

    /// Runs the queued requests in order, then the timed rules, and ends the backend's tick
    /// (§3.3 step 6). Call once per simulation tick.
    pub fn flush(&mut self) {
        for item in std::mem::take(&mut self.queue) {
            match item {
                Queued::Request(request) => self.apply(request),
                Queued::StartBattle(music) => self.begin_battle(music),
                Queued::EndBattle(victory) => self.finish_battle(victory),
                Queued::Focus(focused) => {
                    if focused != self.focused {
                        self.focused = focused;
                        self.send_group(VolumeGroup::Master, self.config.duck_fade_ticks);
                    }
                }
            }
        }
        self.advance_intro();
        self.advance_jingle();
        let backend = &self.backend;
        for voices in self.sfx.values_mut() {
            voices.retain(|v| backend.is_playing(*v));
        }
        self.sfx.retain(|_, voices| !voices.is_empty());
        self.backend.end_tick();
        self.ticks = self.ticks.saturating_add(1);
    }

    fn apply(&mut self, request: AudioRequest) {
        match request {
            AudioRequest::PlayBgm { id, fade } => self.switch_bgm(&id, fade),
            AudioRequest::StopBgm { fade } => {
                if let Some(old) = self.current.take() {
                    let fade = fade.unwrap_or(self.config.crossfade_ticks);
                    self.send(BackendCommand::Stop {
                        voice: old.voice,
                        fade,
                    });
                }
            }
            AudioRequest::PlaySfx { id } => self.sfx(&id),
            AudioRequest::PlayJingle { id } => self.jingle(&id, AfterJingle::Restore),
            AudioRequest::SetVolume { group, volume } => {
                self.volumes[group.index()] = clamp_volume(volume);
                self.send_group(group, 0);
            }
        }
    }

    fn alloc_voice(&mut self) -> VoiceId {
        let voice = VoiceId(self.next_voice);
        self.next_voice = self.next_voice.saturating_add(1);
        voice
    }

    fn send(&mut self, command: BackendCommand) {
        tracing::trace!(target: "audio", tick = self.ticks, %command);
        if let Err(err) = self.backend.execute(&command) {
            tracing::warn!(target: "audio", %command, "audio command failed: {err}");
        }
    }

    fn send_group(&mut self, group: VolumeGroup, fade: u32) {
        let mut volume = self.volumes[group.index()];
        if group == VolumeGroup::Master && !self.focused {
            volume *= clamp_volume(self.config.focus_duck_volume);
        }
        self.send(BackendCommand::SetGroupVolume {
            group,
            volume,
            fade,
        });
    }

    fn held_volume(&self, volume: f32) -> f32 {
        match self.hold {
            Hold::Ducked => volume * clamp_volume(self.config.duck_volume),
            Hold::None | Hold::Paused => volume,
        }
    }

    /// Starts BGM `id` (intro first when it has one) and returns it.
    fn start_bgm(&mut self, id: &str, fade_in: u32) -> Bgm {
        let resolved = self.manifest.resolve_bgm(id);
        let voice = self.alloc_voice();
        let volume = self.held_volume(resolved.volume);
        let (source, pending_main) = match resolved.intro {
            Some(intro) => (intro, Some(resolved.main)),
            None => (resolved.main, None),
        };
        self.send(BackendCommand::Play {
            voice,
            group: VolumeGroup::Bgm,
            source,
            volume,
            fade_in,
        });
        if self.hold == Hold::Paused {
            self.send(BackendCommand::Pause { voice, fade: 0 });
        }
        Bgm {
            id: id.to_owned(),
            voice,
            volume: resolved.volume,
            pending_main,
        }
    }

    fn switch_bgm(&mut self, id: &str, fade: Option<u32>) {
        if self.current.as_ref().is_some_and(|b| b.id == id) {
            return;
        }
        let crossfade = fade.unwrap_or(self.config.crossfade_ticks);
        let old = self.current.take();
        let fade_in = if old.is_some() || fade.is_some() {
            crossfade
        } else {
            0
        };
        if let Some(old) = old {
            self.send(BackendCommand::Stop {
                voice: old.voice,
                fade: crossfade,
            });
        }
        self.current = Some(self.start_bgm(id, fade_in));
    }

    fn sfx(&mut self, id: &str) {
        let (source, max, volume) = self
            .manifest
            .resolve_sfx(id, self.config.default_sfx_instances);
        let mut voices = self.sfx.remove(id).unwrap_or_default();
        voices.retain(|v| self.backend.is_playing(*v));
        while voices.len() >= max as usize {
            let Some(oldest) = voices.pop_front() else {
                break;
            };
            self.send(BackendCommand::Stop {
                voice: oldest,
                fade: 0,
            });
        }
        let voice = self.alloc_voice();
        self.send(BackendCommand::Play {
            voice,
            group: VolumeGroup::Sfx,
            source,
            volume,
            fade_in: 0,
        });
        voices.push_back(voice);
        self.sfx.insert(id.to_owned(), voices);
    }

    fn jingle(&mut self, id: &str, after: AfterJingle) {
        let (source, mode, volume) = self.manifest.resolve_jingle(id);
        let mut after = after;
        if let Some(previous) = self.jingle.take() {
            self.send(BackendCommand::Stop {
                voice: previous.voice,
                fade: 0,
            });
            if previous.after == AfterJingle::ResumeOverworld {
                after = AfterJingle::ResumeOverworld;
            }
        }
        let wanted = match mode {
            JingleMode::Duck => Hold::Ducked,
            JingleMode::Pause => Hold::Paused,
        };
        if wanted > self.hold {
            if let Some(bgm) = self.current.as_ref().map(|b| (b.voice, b.volume)) {
                let (voice, bgm_volume) = bgm;
                match wanted {
                    Hold::Ducked => self.send(BackendCommand::SetVolume {
                        voice,
                        volume: bgm_volume * clamp_volume(self.config.duck_volume),
                        fade: self.config.duck_fade_ticks,
                    }),
                    Hold::Paused => self.send(BackendCommand::Pause { voice, fade: 0 }),
                    Hold::None => {}
                }
            }
            self.hold = wanted;
        }
        let voice = self.alloc_voice();
        self.send(BackendCommand::Play {
            voice,
            group: VolumeGroup::Jingle,
            source,
            volume,
            fade_in: 0,
        });
        self.jingle = Some(Jingle { voice, after });
    }

    /// Lifts the jingle hold on the current BGM.
    fn release_hold(&mut self) {
        let hold = std::mem::replace(&mut self.hold, Hold::None);
        let Some((voice, volume)) = self.current.as_ref().map(|b| (b.voice, b.volume)) else {
            return;
        };
        match hold {
            Hold::Ducked => self.send(BackendCommand::SetVolume {
                voice,
                volume,
                fade: self.config.duck_fade_ticks,
            }),
            Hold::Paused => self.send(BackendCommand::Resume {
                voice,
                fade: self.config.resume_fade_ticks,
            }),
            Hold::None => {}
        }
    }

    fn begin_battle(&mut self, music: Option<String>) {
        if self.in_battle {
            tracing::warn!(target: "audio", "battle music requested while a battle runs");
            if let Some(music) = music {
                self.switch_bgm(&music, Some(0));
            }
            return;
        }
        if let Some(jingle) = self.jingle.take() {
            self.send(BackendCommand::Stop {
                voice: jingle.voice,
                fade: 0,
            });
            // A paused track stays paused for the battle; a ducked one gets its volume back
            // so it resumes at full volume afterwards.
            if self.hold == Hold::Ducked
                && let Some((voice, volume)) = self.current.as_ref().map(|b| (b.voice, b.volume))
            {
                self.send(BackendCommand::SetVolume {
                    voice,
                    volume,
                    fade: 0,
                });
            }
            self.hold = Hold::None;
        }
        self.in_battle = true;
        self.suspended = self.current.take();
        if let Some(voice) = self.suspended.as_ref().map(|b| b.voice) {
            self.send(BackendCommand::Pause { voice, fade: 0 });
        }
        if let Some(music) = music {
            self.current = Some(self.start_bgm(&music, 0));
        }
    }

    fn finish_battle(&mut self, victory: Option<String>) {
        if !self.in_battle {
            return;
        }
        self.in_battle = false;
        if let Some(battle) = self.current.take() {
            let fade = if victory.is_some() {
                0
            } else {
                self.config.battle_fade_ticks
            };
            self.send(BackendCommand::Stop {
                voice: battle.voice,
                fade,
            });
        }
        match victory {
            Some(jingle) => self.jingle(&jingle, AfterJingle::ResumeOverworld),
            None => self.resume_overworld(),
        }
    }

    fn resume_overworld(&mut self) {
        let Some(track) = self.suspended.take() else {
            return;
        };
        if self.current.is_some() {
            // A script started other music meanwhile: it wins.
            self.send(BackendCommand::Stop {
                voice: track.voice,
                fade: 0,
            });
            return;
        }
        match self.config.battle_resume {
            BattleResume::Resume => {
                self.send(BackendCommand::Resume {
                    voice: track.voice,
                    fade: self.config.resume_fade_ticks,
                });
                self.current = Some(track);
            }
            BattleResume::Restart => {
                self.send(BackendCommand::Stop {
                    voice: track.voice,
                    fade: 0,
                });
                self.current = Some(self.start_bgm(&track.id, self.config.resume_fade_ticks));
            }
        }
    }

    /// Hands a finished intro over to its loop.
    fn advance_intro(&mut self) {
        let finished = self
            .current
            .as_ref()
            .is_some_and(|b| b.pending_main.is_some() && !self.backend.is_playing(b.voice));
        if !finished {
            return;
        }
        let Some(mut bgm) = self.current.take() else {
            return;
        };
        if let Some(main) = bgm.pending_main.take() {
            let voice = self.alloc_voice();
            let volume = self.held_volume(bgm.volume);
            self.send(BackendCommand::Play {
                voice,
                group: VolumeGroup::Bgm,
                source: main,
                volume,
                fade_in: 0,
            });
            bgm.voice = voice;
        }
        self.current = Some(bgm);
    }

    /// Ends a finished jingle: the BGM comes back, or the overworld track after a victory.
    fn advance_jingle(&mut self) {
        let finished = self
            .jingle
            .as_ref()
            .is_some_and(|j| !self.backend.is_playing(j.voice));
        if !finished {
            return;
        }
        let Some(jingle) = self.jingle.take() else {
            return;
        };
        self.release_hold();
        if jingle.after == AfterJingle::ResumeOverworld {
            self.resume_overworld();
        }
    }
}

impl std::fmt::Debug for Audio {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Audio")
            .field("backend", &self.backend.name())
            .field("current", &self.current_bgm())
            .field("in_battle", &self.in_battle)
            .field("jingle", &self.jingle.is_some())
            .field("volumes", &self.volumes)
            .field("focused", &self.focused)
            .finish_non_exhaustive()
    }
}
