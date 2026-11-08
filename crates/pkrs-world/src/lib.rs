//! World model: maps, collision, grid movement, warps, connections, NPCs and the `MapImporter` port.
//!
//! - **Model:** [`Map`] separates visual [`TileLayer`]s from the logical [`LogicalGrid`]
//!   (collision, directional blocking, terrain), typed [`MapObject`]s and [`EncounterZone`]s
//!   (§1.4, §6.2).
//! - **Port:** [`MapImporter`] loads a map file through the `pkrs-assets` [`Vfs`](pkrs_assets::Vfs).
//! - **Adapter:** [`TiledImporter`] (feature `tiled`, default) reads `.tmx` + `.tsx` (§6.1).
//! - **Conventions:** [`conventions`] maps layer names and properties onto the model and reports
//!   every [`ConventionViolation`] at once, so `pkrs check` can reuse it; [`diagnostics`] turns
//!   map errors into located §8.2 diagnostics.
//! - **Encounters:** [`encounter`] finds the `encounters` rect and table of a tile and rolls
//!   for a wild encounter after a step.

pub mod conventions;
pub mod diagnostics;
pub mod encounter;
mod error;
mod importer;
mod map;
pub mod movement;
pub mod raw;
pub mod world_layout;

pub use conventions::{ConventionViolation, ViolationKind, ViolationLocation, build_map};
pub use encounter::{
    EncounterSite, WildEncounter, encounter_site, roll_step_encounter, unknown_encounter_tables,
};
pub use error::{Result, WorldError};
#[cfg(feature = "tiled")]
pub use importer::TiledImporter;
pub use importer::{MapError, MapImporter, map_id_for};
pub use map::{
    Cell, EdgeMask, EncounterZone, LogicalGrid, Map, MapObject, MapTileset, ObjectKind, Properties,
    PropertyValue, Terrain, TileLayer, TileRect, TileRef, TilesetImage, Transition,
    VisualLayerKind, Warp,
};
pub use movement::{
    FieldState, GridStepper, LinkCrossing, LockHolder, MovementConfig, Occupancy, PathProgress,
    PlayerLock, ScriptedPath, StepEvent, TerrainQuery, TerrainRuleFn, TerrainRuleRegistry,
    TerrainRules, TerrainStepVerdict,
};
pub use world_layout::{
    MapLinks, NeighborLink, WORLD_RON, WorldLayout, neighbor_pixel_offset, on_departure_edge,
    translate_entry,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: WorldError = WorldError::MapImport("route_1".into());
        assert_eq!(err.to_string(), "failed to import map `route_1`");
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
