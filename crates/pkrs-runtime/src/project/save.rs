//! Save snapshots of the running game (§12): [`ProjectHost::snapshot`] turns the session into
//! [`SaveData`], [`ProjectHost::restore`] puts a save back, and the slot helpers write and read
//! them through a [`SaveStore`].
//!
//! What a snapshot holds:
//! - the overworld's [`WorldState`](pkrs_save::WorldState) (flags, variables, bag, money,
//!   per-map state), after [`WorldModel::sync_map_state`](crate::overworld::WorldModel::sync_map_state)
//!   recorded the active map's NPC changes;
//! - the player's map, tile, facing and field states, the session party and the RNG streams
//!   (battle, wild encounters, the scripts' `math.random`);
//! - every bound plugin's `plugin.save` namespace (values with no saved form are skipped and
//!   logged), plus namespaces of plugins that were in the loaded save but are not enabled now,
//!   so disabling a plugin for a while does not lose its data;
//! - a header with the game identity, play time and a slot-screen summary: `player_name` and
//!   `progress` come from the game variables of the same names when scripts set them.
//!
//! Saving and loading are refused while a battle or a warp runs: a save is only taken from a
//! consistent state between ticks (§9.6). Loading is meant for a host with no script mid-way
//! through a dialogue (e.g. right after start-up, from the title screen).

use std::collections::{BTreeMap, BTreeSet};

use pkrs_core::{GameRng, Pcg64Rng};
use pkrs_platform::UserPaths;
use pkrs_save::{
    CURRENT_FORMAT_VERSION, Loaded, PlayerPlacement, RNG_BATTLE, RNG_ENCOUNTER, RNG_SCRIPT,
    SaveData, SaveError, SaveHeader, SaveLoader, SaveSlot, SaveStore, SaveSummary, SavedCreature,
    Value, WorldState,
};
use pkrs_script::{
    LuaScriptHost, MIGRATE_SCRIPT, ScriptError, ScriptEvent, ScriptFiles, ScriptHost, ScriptLimits,
    ScriptSaveMigration, ScriptValue,
};

use super::{Project, ProjectError, ProjectHost};
use crate::party::{PartyMember, PlayerParty};
use crate::scripts::VfsScriptFiles;

/// Why the game could not be saved or loaded.
#[derive(Debug, thiserror::Error)]
pub enum GameSaveError {
    /// The game is not in a state that can be saved or loaded (a battle or warp is running).
    #[error("cannot save or load {0}")]
    Busy(&'static str),
    /// Writing, reading or decoding the save failed.
    #[error(transparent)]
    Save(#[from] SaveError),
    /// The saved map could not be entered.
    #[error(transparent)]
    Project(#[from] ProjectError),
    /// `scripts/migrate.lua` could not be prepared.
    #[error(transparent)]
    Script(#[from] ScriptError),
}

/// Session data that only exists because a save was loaded.
#[derive(Debug, Default)]
pub(crate) struct SaveSession {
    /// Play time of the loaded save.
    play_time: u64,
    /// Host ticks when it was loaded.
    ticks_at_load: u64,
    /// Plugin namespaces of the loaded save whose plugin is not bound now.
    orphan_plugins: BTreeMap<String, Value>,
}

/// The save slots for `paths`: `UserPaths::data_dir()`, which is `<project>/saves` for
/// `pkrs run` ([`DevPaths`](super::DevPaths)) and the per-OS data directory keyed on
/// `game.id` in a shipped game (§12, §14.3).
#[must_use]
pub fn save_store(paths: &dyn UserPaths) -> SaveStore {
    SaveStore::new(paths.data_dir())
}

/// The game's `scripts/migrate.lua` migration (§12), in a sandboxed script host of its own;
/// `None` when the project has no such script.
pub fn game_migration(
    project: &Project,
) -> Result<Option<ScriptSaveMigration<LuaScriptHost>>, ScriptError> {
    let files = VfsScriptFiles::new(project.vfs.clone());
    if !files.exists(MIGRATE_SCRIPT) {
        return Ok(None);
    }
    let host = LuaScriptHost::new(
        Box::new(files),
        Box::new(Pcg64Rng::from_seed(0)),
        ScriptLimits::default(),
    )?;
    Ok(Some(ScriptSaveMigration::new(host)))
}

fn saved_creature(member: &PartyMember) -> SavedCreature {
    SavedCreature {
        species: member.species.clone(),
        level: member.level,
        exp: member.exp,
        moves: member.moves.clone(),
    }
}

fn party_member(saved: &SavedCreature) -> PartyMember {
    PartyMember {
        species: saved.species.clone(),
        level: saved.level,
        exp: saved.exp,
        moves: saved.moves.clone(),
    }
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl ProjectHost {
    fn ensure_saveable(&self) -> Result<(), GameSaveError> {
        if self.battle.is_some() || self.post_battle.is_some() {
            Err(GameSaveError::Busy("during a battle"))
        } else if self.is_warping() {
            Err(GameSaveError::Busy("during a map transition"))
        } else {
            Ok(())
        }
    }

    /// Total play time in ticks (including the loaded save's).
    #[must_use]
    pub fn play_time_ticks(&self) -> u64 {
        let session = &self.save_session;
        session
            .play_time
            .saturating_add(self.ticks.saturating_sub(session.ticks_at_load))
    }

    /// The running game as [`SaveData`] (§12). Records the active map's NPC changes first
    /// ([`sync_map_state`](crate::overworld::WorldModel::sync_map_state)).
    pub fn snapshot(&mut self) -> Result<SaveData, GameSaveError> {
        self.ensure_saveable()?;
        let map_id = self.scene.map().id.clone();
        let mut placement =
            PlayerPlacement::new(map_id.clone(), self.walker.pos(), self.walker.facing());
        let mut world = match self.scripts.as_ref() {
            Some(scripts) => {
                let mut model = scripts.world().get_mut();
                model.sync_map_state();
                placement.field_states = model.field_states.clone();
                model.state.clone()
            }
            None => WorldState::new(),
        };
        world.player = Some(placement);
        world.party = self
            .party
            .as_ref()
            .map(|p| p.members().iter().map(saved_creature).collect())
            .unwrap_or_default();
        world.rng = BTreeMap::from([
            (RNG_BATTLE.to_owned(), self.battle_rng.state()),
            (RNG_ENCOUNTER.to_owned(), self.encounters.rng_state()),
        ]);
        if let Some(state) = self.scripts.as_ref().and_then(|s| s.host().rng_state()) {
            world.rng.insert(RNG_SCRIPT.to_owned(), state);
        }

        let manifest = &self.project.manifest;
        let summary = SaveSummary {
            player_name: world
                .var("player_name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            location: map_id,
            progress: world
                .var("progress")
                .and_then(Value::as_int)
                .and_then(|p| u32::try_from(p).ok())
                .unwrap_or(0),
            party_size: u32::try_from(world.party.len()).unwrap_or(u32::MAX),
        };
        let mut save = SaveData {
            header: SaveHeader {
                format_version: CURRENT_FORMAT_VERSION,
                engine_version: env!("CARGO_PKG_VERSION").to_owned(),
                game_id: manifest.game.id.clone(),
                game_version: manifest.game.version.to_string(),
                timestamp: unix_seconds(),
                play_time_ticks: self.play_time_ticks(),
                summary,
            },
            world,
            plugins: self.save_session.orphan_plugins.clone(),
        };
        if let Some(scripts) = self.scripts.as_ref() {
            // `plugin_saves` already skipped (and logged) functions, userdata and cycles.
            for (id, value) in scripts.host().plugin_saves() {
                match value.to_save_value() {
                    Some(value) => save.set_plugin(&id, value),
                    None => {
                        save.plugins.remove(&id);
                    }
                }
            }
        }
        Ok(save)
    }

    /// Puts `save` back (§12): world state, party, RNG streams and plugin namespaces, then
    /// enters the saved map at the saved tile (a save without a position keeps the current
    /// map and tile). The previous map is exited first, so its `map_exit` listeners run.
    pub fn restore(&mut self, save: &SaveData) -> Result<(), GameSaveError> {
        self.ensure_saveable()?;
        if !save.header.game_id.is_empty() && save.header.game_id != self.project.manifest.game.id {
            tracing::warn!(target: "save", saved = %save.header.game_id, game = %self.project.manifest.game.id, "loading a save of another game id");
        }
        let placement = save.world.player.clone().unwrap_or_else(|| {
            PlayerPlacement::new(
                self.scene.map().id.clone(),
                self.walker.pos(),
                self.walker.facing(),
            )
        });
        let map = self.project.load_map_by_id(&placement.map)?;
        if !map.grid.in_bounds(placement.pos()) {
            return Err(ProjectError::StartOutOfBounds {
                map: map.id.clone(),
                pos: placement.pos(),
                width: map.width,
                height: map.height,
            }
            .into());
        }

        self.party = (!save.world.party.is_empty())
            .then(|| PlayerParty::new(save.world.party.iter().map(party_member).collect()));
        if let Some(state) = save.world.rng.get(RNG_BATTLE) {
            self.battle_rng.restore_state(state.clone());
        }
        if let Some(state) = save.world.rng.get(RNG_ENCOUNTER) {
            self.encounters.restore_rng_state(state.clone());
        }

        let mut orphans = BTreeMap::new();
        if let Some(scripts) = self.scripts.as_mut() {
            if let Some(state) = save.world.rng.get(RNG_SCRIPT) {
                scripts.host_mut().restore_rng_state(state.clone());
            }
            let bound: BTreeSet<String> = scripts.host().plugin_saves().into_keys().collect();
            for (id, value) in &save.plugins {
                if !bound.contains(id) {
                    tracing::info!(target: "save", plugin = %id, "save data of a plugin that is not enabled is kept");
                    orphans.insert(id.clone(), value.clone());
                    continue;
                }
                let value = ScriptValue::from_save_value(Some(value));
                if let Err(err) = scripts.host_mut().restore_plugin_save(id, &value) {
                    tracing::warn!(target: "save", plugin = %id, "plugin save not restored: {err}");
                }
            }
            scripts.exit_map();
            let mut model = scripts.world().get_mut();
            model.state = save.world.clone();
            model.field_states = placement.field_states.clone();
        } else {
            orphans = save.plugins.clone();
        }
        self.enter_loaded_map(map, placement.pos(), placement.facing)?;
        self.save_session = SaveSession {
            play_time: save.header.play_time_ticks,
            ticks_at_load: self.ticks,
            orphan_plugins: orphans,
        };
        tracing::info!(target: "save", map = %placement.map, pos = %placement.pos(), "save restored");
        Ok(())
    }

    /// The save slots of this project (`<project>/saves` for `pkrs run`, §12).
    #[must_use]
    pub fn save_store(&self) -> SaveStore {
        save_store(&self.project.paths)
    }

    /// Takes a [`snapshot`](Self::snapshot) and writes it into `slot` atomically.
    pub fn save_to_slot(&mut self, slot: SaveSlot) -> Result<SaveData, GameSaveError> {
        self.ensure_saveable()?;
        if let Some(scripts) = self.scripts.as_mut() {
            scripts
                .host_mut()
                .emit_event(ScriptEvent::save_before(&slot.to_string()));
            scripts.flush_events();
        }
        let save = self.snapshot()?;
        self.save_store().write(slot, &save)?;
        Ok(save)
    }

    /// Loads `slot` (engine migrations, then the game's `scripts/migrate.lua` if the save was
    /// written by another game version; the `.bak` if the save is unreadable) and
    /// [`restore`](Self::restore)s it.
    pub fn load_from_slot(&mut self, slot: SaveSlot) -> Result<Loaded, GameSaveError> {
        self.ensure_saveable()?;
        let mut migration = game_migration(&self.project)?;
        let game_version = self.project.manifest.game.version.to_string();
        let mut loader = SaveLoader::new();
        if let Some(migration) = migration.as_mut() {
            loader = loader.with_game_migration(migration, game_version);
        }
        let loaded = self.save_store().load(slot, &mut loader)?;
        self.restore(&loaded.data)?;
        self.game_started = true;
        if let Some(scripts) = self.scripts.as_mut() {
            scripts.host_mut().emit_event(ScriptEvent::load_after(
                &slot.to_string(),
                loaded.from_backup,
            ));
            scripts.flush_events();
        }
        Ok(loaded)
    }
}
