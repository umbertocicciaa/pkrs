//! Assets facade: virtual file system (loose folder or `.pak`), handles and caching (§13).
//!
//! - **Port:** [`Vfs`] (`read`, `exists`, `list`) over normalised [`VfsPath`]s.
//! - **Adapters:** [`FolderVfs`] (loose files, `pkrs run`) and [`MemoryVfs`] (tests, headless).
//!   `PakVfs` (zip) lands with `.pak` support (M7).
//! - **Facade:** [`Assets`] routes `plugin_id:` prefixes through a mount table and caches
//!   decoded assets per type in [`AssetStore`]s, handing out generational [`Handle`]s with
//!   reference counting for eviction on map change.
//! - **Paths:** see [`path`] for normalisation, traversal and case rules.

mod assets;
mod cache;
mod error;
mod handle;
pub mod path;
mod vfs;

pub use assets::{AssetEntry, Assets};
pub use cache::{Asset, AssetStore, RawBytes, Text};
pub use error::{AssetsError, Result};
pub use handle::{Handle, SlotMap};
pub use path::{AssetPath, VfsPath};
pub use vfs::{EntryKind, FolderVfs, MemoryVfs, Vfs, VfsEntry};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display_is_human_readable() {
        let err: AssetsError = AssetsError::NotFound("gfx/hero.png".into());
        assert_eq!(err.to_string(), "asset not found: gfx/hero.png");
        let err = AssetsError::CaseMismatch {
            requested: "sprites/hero.png".into(),
            actual: "Sprites/Hero.png".into(),
        };
        assert_eq!(
            err.to_string(),
            "asset path `sprites/hero.png` does not match the on-disk case `Sprites/Hero.png`"
        );
    }

    #[test]
    fn result_alias_defaults_to_crate_error() {
        let ok: Result<u8> = Ok(1);
        assert!(matches!(ok, Ok(1)));
    }
}
