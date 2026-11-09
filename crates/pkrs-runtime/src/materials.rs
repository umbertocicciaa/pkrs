//! Loads the materials a map references (§5.6): `assets/materials/<id>.ron` through the VFS,
//! project shaders under `assets/`, compiled once per id through the [`Renderer`].
//!
//! A material that is missing, invalid (the same rules as `pkrs check`) or that the backend
//! cannot compile is logged once and the tiles or sprites draw without it.

use std::collections::BTreeMap;

use pkrs_assets::{FolderVfs, Vfs, VfsPath};
use pkrs_render::{Material, MaterialHandle, Renderer, material_path};
use pkrs_world::Map;

/// Material handles by id (`None` = failed to load, not retried).
#[derive(Debug, Default)]
pub struct MaterialCache {
    by_id: BTreeMap<String, Option<MaterialHandle>>,
}

/// Materials of one map, ready for [`MapScene::set_materials`](crate::MapScene::set_materials).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapMaterials {
    /// Per tileset, indexed like `Map::tilesets`.
    pub tilesets: Vec<Option<MaterialHandle>>,
    /// Per visual layer, indexed like `Map::layers`.
    pub layers: Vec<Option<MaterialHandle>>,
}

impl MaterialCache {
    /// Empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The handle of material `id`, loading and compiling it on first use.
    pub fn get(
        &mut self,
        id: &str,
        vfs: &FolderVfs,
        renderer: &mut Renderer,
    ) -> Option<MaterialHandle> {
        if let Some(found) = self.by_id.get(id) {
            return *found;
        }
        let handle = match load(id, vfs) {
            Ok(material) => match renderer.create_material(id, material) {
                Ok(handle) => Some(handle),
                Err(err) => {
                    tracing::warn!(target: "render", material = id, %err, "material cannot be created; drawing without it");
                    None
                }
            },
            Err(err) => {
                tracing::warn!(target: "render", material = id, %err, "material not loaded; drawing without it");
                None
            }
        };
        self.by_id.insert(id.to_owned(), handle);
        handle
    }

    /// The materials referenced by `map`'s tilesets and layers (`material` properties).
    pub fn for_map(&mut self, map: &Map, vfs: &FolderVfs, renderer: &mut Renderer) -> MapMaterials {
        MapMaterials {
            tilesets: map
                .tilesets
                .iter()
                .map(|ts| {
                    ts.material
                        .as_deref()
                        .and_then(|id| self.get(id, vfs, renderer))
                })
                .collect(),
            layers: map
                .layers
                .iter()
                .map(|l| {
                    l.material
                        .as_deref()
                        .and_then(|id| self.get(id, vfs, renderer))
                })
                .collect(),
        }
    }
}

fn read_text(vfs: &FolderVfs, path: &str) -> Option<String> {
    let path = VfsPath::new(path).ok()?;
    let bytes = vfs.read(&path).ok()?;
    String::from_utf8(bytes).ok()
}

fn load(id: &str, vfs: &FolderVfs) -> Result<Material, String> {
    let path = material_path(id);
    let source = read_text(vfs, &path).ok_or_else(|| format!("cannot read `{path}`"))?;
    let source = source.replace("\r\n", "\n");
    Material::from_ron(&source, &|p| read_text(vfs, p)).map_err(|e| format!("{path}: {e}"))
}
