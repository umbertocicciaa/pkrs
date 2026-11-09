//! Maps changed project paths to reload actions (§8.3).

use super::WatchEvent;

/// What subsystem a file change should refresh at the next tick boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReloadKind {
    /// Not watched (e.g. saves, editor metadata).
    Ignore,
    /// Lua under `scripts/` or a plugin `init.lua`.
    Scripts,
    /// RON under `data/` or localized `text/`.
    Data,
    /// Images and tilesets used by the renderer.
    Textures,
}

/// What the runtime should run once, after coalescing file events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReloadAction {
    /// Re-run touched scripts (map script, `main.lua`, plugins).
    Scripts,
    /// Re-validate and swap game data.
    Data,
    /// Reload textures for the active map.
    Textures,
}

/// Classifies a normalised project-relative path.
#[must_use]
pub fn reload_kind(path: &str) -> ReloadKind {
    if path.ends_with(".lua")
        && (path.starts_with("scripts/")
            || (path.starts_with("plugins/") && path.ends_with("init.lua")))
    {
        return ReloadKind::Scripts;
    }
    if path.starts_with("data/") || path.starts_with("text/") {
        return ReloadKind::Data;
    }
    if path.starts_with("assets/")
        || path.starts_with("sprites/")
        || path.starts_with("tilesets/")
        || path.ends_with(".png")
        || path.ends_with(".tsx")
    {
        return ReloadKind::Textures;
    }
    ReloadKind::Ignore
}

/// Coalesces watcher events into actions (scripts beat data beat textures).
#[must_use]
pub fn reload_actions(events: &[WatchEvent]) -> Vec<ReloadAction> {
    let mut scripts = false;
    let mut data = false;
    let mut textures = false;
    for event in events {
        match reload_kind(&event.path) {
            ReloadKind::Ignore => {}
            ReloadKind::Scripts => scripts = true,
            ReloadKind::Data => data = true,
            ReloadKind::Textures => textures = true,
        }
    }
    let mut out = Vec::new();
    if scripts {
        out.push(ReloadAction::Scripts);
    }
    if data {
        out.push(ReloadAction::Data);
    }
    if textures {
        out.push(ReloadAction::Textures);
    }
    out
}

/// `true` when `path` is the active map's `scripts/maps/<id>.lua`.
#[must_use]
pub fn is_map_script(path: &str, map_id: &str) -> bool {
    path == format!("scripts/maps/{map_id}.lua")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::WatchChange;

    #[test]
    fn classifies_script_data_and_texture_paths() {
        assert_eq!(reload_kind("scripts/maps/town.lua"), ReloadKind::Scripts);
        assert_eq!(
            reload_kind("plugins/resonance/init.lua"),
            ReloadKind::Scripts
        );
        assert_eq!(reload_kind("data/species/a.ron"), ReloadKind::Data);
        assert_eq!(reload_kind("sprites/player.png"), ReloadKind::Textures);
        assert_eq!(reload_kind("saves/slot1.sav"), ReloadKind::Ignore);
    }

    #[test]
    fn merge_prefers_each_kind_once() {
        let events = [
            WatchEvent {
                path: "scripts/main.lua".into(),
                change: WatchChange::Modify,
            },
            WatchEvent {
                path: "data/types.ron".into(),
                change: WatchChange::Modify,
            },
        ];
        assert_eq!(
            reload_actions(&events),
            [ReloadAction::Scripts, ReloadAction::Data]
        );
    }
}
