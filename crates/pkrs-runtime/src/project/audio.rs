//! Game audio of a running project (§11): the [`Audio`] facade wired to the overworld.
//!
//! - **Map BGM.** Entering a map (start, warp, seamless connection, loading a save) queues
//!   the map's `bgm` property; the facade keeps the track when the id is unchanged and
//!   crossfades otherwise (30 ticks). A map without `bgm` leaves the music alone.
//! - **Battles.** A battle start pauses the overworld track and plays the manifest's wild or
//!   trainer battle music; when the result is known, the battle track stops, the victory
//!   jingle plays on a win and the overworld track resumes after it.
//! - **Scripts.** `audio.*` requests are queued in call order with the engine's own.
//! - **Focus.** Focus changes duck the master volume (§3.2).
//! - **Flush.** Once per tick, last (§3.3 step 6).
//!
//! The host starts on [`NullAudio`] (headless runs and tests read its [`AudioLog`]); the
//! windowed runtime swaps in the kira adapter with [`ProjectHost::set_audio_backend`].

use pkrs_audio::{
    Audio, AudioBackend, AudioConfig, AudioLog, AudioManifest, AudioRequest, BattleKind, NullAudio,
};
use pkrs_battle::{BattleOutcome, BattleResult};
use pkrs_script::BattleRequest;
use pkrs_world::Map;

use super::{Project, ProjectHost};

/// The facade plus what the host tracks around it.
pub(crate) struct ProjectAudio {
    audio: Audio,
    /// The command log while the backend is the default [`NullAudio`].
    log: Option<AudioLog>,
    /// Kind of the running battle (picks the victory jingle).
    battle: Option<BattleKind>,
    focused: bool,
}

impl ProjectAudio {
    /// The project's manifest (an unreadable one is logged and replaced by defaults) over a
    /// [`NullAudio`].
    pub(crate) fn for_project(project: &Project) -> Self {
        let manifest = AudioManifest::load(&project.vfs).unwrap_or_else(|err| {
            tracing::error!(target: "audio", "audio manifest ignored: {err}");
            AudioManifest::default()
        });
        let null = NullAudio::new();
        let log = null.log();
        Self {
            audio: Audio::new(Box::new(null), manifest, AudioConfig::default()),
            log: Some(log),
            battle: None,
            focused: true,
        }
    }

    /// Queues the `bgm` of the map just entered.
    pub(crate) fn enter_map(&mut self, map: &Map) {
        match map.properties.string("bgm") {
            Ok(id) => self.audio.map_bgm(id.filter(|id| !id.is_empty())),
            Err(found) => {
                tracing::warn!(target: "audio", map = %map.id, "map property `bgm` must be a string (got {found})");
            }
        }
    }

    /// Queues the battle music for `request`.
    pub(crate) fn start_battle(&mut self, request: &BattleRequest) {
        let kind = match request {
            BattleRequest::Wild { .. } => BattleKind::Wild,
            BattleRequest::Trainer { .. } => BattleKind::Trainer,
        };
        self.battle = Some(kind);
        self.audio.start_battle(kind);
    }

    /// Queues the end of the battle music for `result`.
    pub(crate) fn end_battle(&mut self, result: &BattleResult) {
        let kind = self.battle.take().unwrap_or(BattleKind::Wild);
        let won = matches!(result.outcome, BattleOutcome::PlayerWin);
        self.audio.end_battle(kind, won);
    }

    /// Queues a script's request.
    pub(crate) fn request(&mut self, request: AudioRequest) {
        self.audio.request(request);
    }

    /// Queues a focus change, if it changed.
    pub(crate) fn set_focus(&mut self, focused: bool) {
        if focused != self.focused {
            self.focused = focused;
            self.audio.set_focus(focused);
        }
    }

    /// §3.3 step 6.
    pub(crate) fn flush(&mut self) {
        self.audio.flush();
    }
}

impl ProjectHost {
    /// The audio facade.
    #[must_use]
    pub fn audio(&self) -> &Audio {
        &self.audio.audio
    }

    /// The audio facade, to queue requests from engine code.
    pub fn audio_mut(&mut self) -> &mut Audio {
        &mut self.audio.audio
    }

    /// Commands the default [`NullAudio`] received (`None` once another backend is set): the
    /// headless harness asserts on it (§15.2).
    #[must_use]
    pub fn audio_log(&self) -> Option<&AudioLog> {
        self.audio.log.as_ref()
    }

    /// Plays through `backend` from now on (the windowed runtime's kira adapter). Call it
    /// before the first tick: voices already started on the previous backend are not moved.
    pub fn set_audio_backend(&mut self, backend: Box<dyn AudioBackend>) {
        if self.audio.audio.ticks() > 0 {
            tracing::warn!(target: "audio", "audio backend replaced after the first tick");
        }
        self.audio.audio.set_backend(backend);
        self.audio.log = None;
    }
}
