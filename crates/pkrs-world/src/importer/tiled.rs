//! `TiledImporter`: the `tiled` crate adapter for the [`MapImporter`] port (§4.5, §6.1).
//!
//! This module is the only place in the workspace that names `tiled` types. It reads every file
//! through the engine [`Vfs`] (never the OS file system), translates the Tiled map into a
//! vendor-free [`RawMap`], and hands it to [`build_map`] for the §6.2 conventions.
//! Only `.tmx` maps with external or embedded `.tsx` tilesets are supported (§17 #2).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::Path;

use pkrs_assets::{AssetsError, Vfs, VfsPath};

use super::{MapError, MapImporter, map_id_for};
use crate::conventions::build_map;
use crate::map::{Map, Properties, PropertyValue, TileRef, TilesetImage};
use crate::raw::{RawLayer, RawMap, RawObject, RawOrientation, RawTileset};

/// Loads Tiled `.tmx` maps (and their `.tsx` tilesets) through a [`Vfs`].
#[derive(Clone, Copy, Debug, Default)]
pub struct TiledImporter;

impl TiledImporter {
    /// Creates the importer.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl MapImporter for TiledImporter {
    fn import(&mut self, vfs: &dyn Vfs, path: &VfsPath) -> Result<Map, MapError> {
        if !path.as_str().ends_with(".tmx") {
            return Err(MapError::Parse {
                path: path.as_str().to_owned(),
                message: "only Tiled `.tmx` maps are supported".into(),
            });
        }
        let reader = VfsReader {
            vfs,
            failure: RefCell::new(None),
        };
        let mut loader = tiled::Loader::with_reader(reader);
        let loaded = loader.load_tmx_map(path.as_str());
        let tmx = match loaded {
            Ok(map) => map,
            Err(err) => {
                let (_, reader) = loader.into_inner();
                if let Some((failed, source)) = reader.failure.into_inner() {
                    return Err(MapError::Read {
                        path: failed,
                        source,
                    });
                }
                return Err(MapError::Parse {
                    path: path.as_str().to_owned(),
                    message: err.to_string(),
                });
            }
        };
        let raw = to_raw(&tmx, map_id_for(path)).map_err(|message| MapError::Parse {
            path: path.as_str().to_owned(),
            message,
        })?;
        build_map(raw).map_err(|violations| MapError::Conventions {
            path: path.as_str().to_owned(),
            violations,
        })
    }
}

/// `tiled::ResourceReader` over the engine VFS. Remembers the first VFS failure so it can be
/// reported as [`MapError::Read`] instead of an opaque parse error.
struct VfsReader<'a> {
    vfs: &'a dyn Vfs,
    failure: RefCell<Option<(String, AssetsError)>>,
}

impl tiled::ResourceReader for VfsReader<'_> {
    type Resource = Cursor<Vec<u8>>;
    type Error = std::io::Error;

    fn read_from(&mut self, path: &Path) -> Result<Self::Resource, Self::Error> {
        let raw = path.to_string_lossy();
        match VfsPath::new(&raw).and_then(|p| self.vfs.read(&p)) {
            Ok(bytes) => Ok(Cursor::new(bytes)),
            Err(err) => {
                let message = err.to_string();
                let mut failure = self.failure.borrow_mut();
                if failure.is_none() {
                    *failure = Some((raw.into_owned(), err));
                }
                Err(std::io::Error::other(message))
            }
        }
    }
}

fn to_raw(tmx: &tiled::Map, id: String) -> Result<RawMap, String> {
    let tilesets = tmx
        .tilesets()
        .iter()
        .map(|ts| to_raw_tileset(ts))
        .collect::<Result<Vec<_>, _>>()?;
    let mut layers = Vec::new();
    for layer in tmx.layers() {
        layers.push(to_raw_layer(&layer)?);
    }
    Ok(RawMap {
        id,
        orientation: match tmx.orientation {
            tiled::Orientation::Orthogonal => RawOrientation::Orthogonal,
            _ => RawOrientation::Other,
        },
        infinite: tmx.infinite(),
        width: tmx.width,
        height: tmx.height,
        tile_width: tmx.tile_width,
        tile_height: tmx.tile_height,
        properties: to_properties(&tmx.properties),
        tilesets,
        layers,
    })
}

fn to_raw_tileset(ts: &tiled::Tileset) -> Result<RawTileset, String> {
    let image = match &ts.image {
        None => None,
        Some(img) => {
            let raw = img.source.to_string_lossy();
            let path =
                VfsPath::new(&raw).map_err(|e| format!("tileset `{}` image: {e}", ts.name))?;
            Some(TilesetImage {
                path: path.as_str().to_owned(),
                width: u32::try_from(img.width).unwrap_or(0),
                height: u32::try_from(img.height).unwrap_or(0),
            })
        }
    };
    let tile_properties: BTreeMap<u32, Properties> = ts
        .tiles()
        .filter(|(_, tile)| !tile.properties.is_empty())
        .map(|(id, tile)| (id, to_properties(&tile.properties)))
        .collect();
    Ok(RawTileset {
        name: ts.name.clone(),
        tile_width: ts.tile_width,
        tile_height: ts.tile_height,
        columns: ts.columns,
        tile_count: ts.tilecount,
        spacing: ts.spacing,
        margin: ts.margin,
        image,
        tile_properties,
        properties: to_properties(&ts.properties),
    })
}

fn to_raw_layer(layer: &tiled::Layer<'_>) -> Result<RawLayer, String> {
    let name = layer.name.clone();
    let visible = layer.visible;
    Ok(match layer.layer_type() {
        tiled::LayerType::Tiles(tiled::TileLayer::Finite(finite)) => {
            let (w, h) = (finite.width(), finite.height());
            let mut tiles = Vec::with_capacity((w as usize).saturating_mul(h as usize));
            for y in 0..h {
                for x in 0..w {
                    let (Ok(xi), Ok(yi)) = (i32::try_from(x), i32::try_from(y)) else {
                        return Err(format!("layer `{name}` is too large"));
                    };
                    tiles.push(match finite.get_tile_data(xi, yi) {
                        None => None,
                        Some(t) => Some(TileRef {
                            tileset: u16::try_from(t.tileset_index())
                                .map_err(|_| "too many tilesets".to_string())?,
                            id: t.id(),
                            flip_h: t.flip_h,
                            flip_v: t.flip_v,
                            flip_d: t.flip_d,
                        }),
                    });
                }
            }
            RawLayer::Tiles {
                name,
                visible,
                tiles,
                properties: to_properties(&layer.properties),
            }
        }
        tiled::LayerType::Tiles(tiled::TileLayer::Infinite(_)) => RawLayer::Unsupported {
            name,
            kind: "infinite tile layer",
        },
        tiled::LayerType::Objects(objects) => RawLayer::Objects {
            name,
            objects: objects.objects().map(|o| to_raw_object(&o)).collect(),
        },
        tiled::LayerType::Image(_) => RawLayer::Unsupported {
            name,
            kind: "image layer",
        },
        tiled::LayerType::Group(_) => RawLayer::Unsupported {
            name,
            kind: "group layer",
        },
    })
}

fn to_raw_object(obj: &tiled::Object<'_>) -> RawObject {
    let (width, height) = match &obj.shape {
        tiled::ObjectShape::Rect { width, height }
        | tiled::ObjectShape::Ellipse { width, height }
        | tiled::ObjectShape::Capsule { width, height }
        | tiled::ObjectShape::Text { width, height, .. } => (*width, *height),
        tiled::ObjectShape::Point(..)
        | tiled::ObjectShape::Polyline { .. }
        | tiled::ObjectShape::Polygon { .. } => (0.0, 0.0),
    };
    // Tile objects are anchored at their bottom-left corner.
    let y = if obj.tile_data().is_some() {
        obj.y - height
    } else {
        obj.y
    };
    RawObject {
        id: obj.id(),
        name: obj.name.clone(),
        class: obj.user_type.clone(),
        x: obj.x,
        y,
        width,
        height,
        properties: to_properties(&obj.properties),
    }
}

fn to_properties(props: &tiled::Properties) -> Properties {
    props
        .iter()
        .map(|(k, v)| (k.clone(), to_value(v)))
        .collect()
}

fn to_value(value: &tiled::PropertyValue) -> PropertyValue {
    match value {
        tiled::PropertyValue::BoolValue(b) => PropertyValue::Bool(*b),
        tiled::PropertyValue::IntValue(i) => PropertyValue::Int(i64::from(*i)),
        tiled::PropertyValue::FloatValue(f) => PropertyValue::Float(f64::from(*f)),
        tiled::PropertyValue::StringValue(s) => PropertyValue::String(s.clone()),
        tiled::PropertyValue::FileValue(s) => PropertyValue::File(s.clone()),
        tiled::PropertyValue::ColorValue(c) => {
            PropertyValue::Color([c.red, c.green, c.blue, c.alpha])
        }
        tiled::PropertyValue::ObjectValue(id) => PropertyValue::Object(*id),
        tiled::PropertyValue::ListValue(items) => {
            PropertyValue::List(items.iter().map(to_value).collect())
        }
        tiled::PropertyValue::ClassValue {
            property_type,
            properties,
        } => PropertyValue::Class {
            class: property_type.clone(),
            properties: to_properties(properties),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::{ObjectKind, Terrain, VisualLayerKind};
    use pkrs_assets::MemoryVfs;
    use pkrs_core::{Direction, GridPos};

    const TSX: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<tileset version="1.10" name="basic" tilewidth="16" tileheight="16" tilecount="4" columns="2">
 <image source="basic.png" width="32" height="32"/>
 <tile id="1"><properties><property name="solid" type="bool" value="true"/></properties></tile>
 <tile id="2"><properties><property name="terrain" value="tall_grass"/></properties></tile>
 <tile id="3"><properties><property name="block_n" type="bool" value="true"/></properties></tile>
</tileset>
"#;

    fn tmx(layers: &str) -> String {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<map version="1.10" orientation="orthogonal" renderorder="right-down" width="3" height="2" tilewidth="16" tileheight="16" infinite="0" nextlayerid="9" nextobjectid="9">
 <properties><property name="bgm" value="town"/></properties>
 <tileset firstgid="1" source="../tilesets/basic.tsx"/>
{layers}
</map>
"#
        )
    }

    const GROUND: &str = r#" <layer id="1" name="ground" width="3" height="2"><data encoding="csv">2,3,1,1,1,4</data></layer>"#;

    fn vfs_with(map: &str) -> MemoryVfs {
        let mut vfs = MemoryVfs::new();
        vfs.insert("maps/town.tmx", map.as_bytes().to_vec())
            .unwrap();
        vfs.insert("tilesets/basic.tsx", TSX.as_bytes().to_vec())
            .unwrap();
        vfs
    }

    fn import(vfs: &MemoryVfs, path: &str) -> Result<Map, MapError> {
        TiledImporter::new().import(vfs, &VfsPath::new(path).unwrap())
    }

    #[test]
    fn imports_layers_grid_and_tileset_paths() {
        let vfs = vfs_with(&tmx(GROUND));
        let map = import(&vfs, "maps/town.tmx").unwrap();
        assert_eq!(map.id, "town");
        assert_eq!((map.width, map.height), (3, 2));
        assert_eq!(map.layers[0].kind, VisualLayerKind::Ground);
        assert_eq!(map.layers[0].tile(1, 0), Some(TileRef::new(0, 2)));
        assert_eq!(
            map.tilesets[0].image.as_ref().unwrap().path,
            "tilesets/basic.png",
            "image path is normalised relative to the project root"
        );
        assert_eq!(
            map.properties.string("bgm"),
            Ok(Some("town")),
            "map properties are kept"
        );
        assert!(
            map.grid.is_solid(GridPos::new(0, 0)),
            "tile 1 is solid=true"
        );
        assert_eq!(
            map.grid.terrain(GridPos::new(1, 0)),
            Some(&Terrain::TallGrass)
        );
        assert!(
            !map.grid.can_cross(GridPos::new(2, 0), Direction::Down),
            "tile 3 blocks north edge of (2,1)"
        );
    }

    #[test]
    fn imports_objects_and_encounters() {
        let layers = format!(
            "{GROUND}\n{}",
            r#" <objectgroup id="2" name="objects">
  <object id="1" name="door" type="warp" x="16" y="16" width="16" height="16">
   <properties>
    <property name="to_map" value="house"/>
    <property name="to_x" type="int" value="2"/>
    <property name="to_y" type="int" value="5"/>
   </properties>
  </object>
  <object id="2" name="start" type="spawn" x="0" y="0"><point/></object>
 </objectgroup>
 <objectgroup id="3" name="encounters">
  <object id="3" x="0" y="0" width="48" height="16">
   <properties>
    <property name="table" value="town_grass"/>
    <property name="rate" type="int" value="15"/>
   </properties>
  </object>
 </objectgroup>"#
        );
        let vfs = vfs_with(&tmx(&layers));
        let map = import(&vfs, "maps/town.tmx").unwrap();
        assert_eq!(map.objects.len(), 2);
        assert!(
            matches!(&map.objects[0].kind, ObjectKind::Warp(w) if w.to_map == "house" && w.to == GridPos::new(2, 5))
        );
        assert_eq!(map.objects[0].pos(), GridPos::new(1, 1));
        assert_eq!(map.spawn("start").unwrap().pos(), GridPos::new(0, 0));
        assert_eq!(map.encounters[0].table, "town_grass");
        assert_eq!(map.encounters[0].rect.width, 3);
    }

    #[test]
    fn convention_violations_surface_as_structured_errors() {
        let layers = r#" <layer id="1" name="floor" width="3" height="2"><data encoding="csv">1,1,1,1,1,1</data></layer>"#;
        let vfs = vfs_with(&tmx(layers));
        let err = import(&vfs, "maps/town.tmx").unwrap_err();
        assert_eq!(err.violations().len(), 2, "{err}");
    }

    #[test]
    fn missing_tileset_is_a_read_error() {
        let mut vfs = MemoryVfs::new();
        vfs.insert("maps/town.tmx", tmx(GROUND).into_bytes())
            .unwrap();
        let err = import(&vfs, "maps/town.tmx").unwrap_err();
        assert!(
            matches!(&err, MapError::Read { path, source: AssetsError::NotFound(_) } if path.ends_with("basic.tsx")),
            "{err:?}"
        );
    }

    #[test]
    fn malformed_xml_is_a_parse_error() {
        let vfs = vfs_with("<map");
        assert!(matches!(
            import(&vfs, "maps/town.tmx"),
            Err(MapError::Parse { .. })
        ));
    }

    #[test]
    fn non_tmx_files_are_rejected() {
        let vfs = vfs_with(&tmx(GROUND));
        assert!(matches!(
            import(&vfs, "maps/town.tmj"),
            Err(MapError::Parse { .. })
        ));
    }
}
