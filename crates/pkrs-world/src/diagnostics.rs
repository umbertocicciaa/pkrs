//! Map problems as vendor-free [`Diagnostic`]s (§6.1, §8.2): import failures and every
//! [`ConventionViolation`], located in the `.tmx` source when it is at hand.
//!
//! The locators search the Tiled XML text (`<object id="3"`, `<property name="script"`,
//! `name="ground"`), so they work for any importer whose files keep Tiled's attribute
//! layout.

use pkrs_core::diagnostic::find_from;
use pkrs_core::{Diagnostic, SourceSpan};

use crate::conventions::{ConventionViolation, ViolationKind, ViolationLocation};
use crate::importer::MapError;

/// Code of map files that cannot be read.
pub const CODE_READ: &str = "map::read";
/// Code of map files that are not valid Tiled maps.
pub const CODE_PARSE: &str = "map::parse";
/// Code of §6.2 layer-convention violations.
pub const CODE_CONVENTION: &str = "map::convention";

/// Every problem of `err` (the import of `file`) as a diagnostic; `source` is the `.tmx`
/// text, used for spans.
#[must_use]
pub fn map_error_diagnostics(err: &MapError, file: &str, source: Option<&str>) -> Vec<Diagnostic> {
    match err {
        MapError::Read { path, source: e } => {
            vec![Diagnostic::error(CODE_READ, format!("cannot read `{path}`: {e}")).in_file(file)]
        }
        MapError::Parse { message, .. } => {
            vec![
                Diagnostic::error(CODE_PARSE, format!("not a valid Tiled map: {message}"))
                    .in_file(file),
            ]
        }
        MapError::Conventions { violations, .. } => violations
            .iter()
            .map(|v| violation_diagnostic(v, file, source))
            .collect(),
    }
}

/// One violation as a diagnostic in `file`.
#[must_use]
pub fn violation_diagnostic(
    v: &ConventionViolation,
    file: &str,
    source: Option<&str>,
) -> Diagnostic {
    let span = source.and_then(|src| locate_violation(src, v));
    let help = match &v.kind {
        ViolationKind::MissingLayer(_) | ViolationKind::UnknownLayer { .. } => Some(
            "layers are ground, ground2…, overhead, collision, terrain, objects and encounters (§6.2)",
        ),
        ViolationKind::UnknownObjectClass(_) => Some("set the object's class (Tiled 1.9+) or type"),
        _ => None,
    };
    let diagnostic = Diagnostic::error(CODE_CONVENTION, v.to_string())
        .in_file(file)
        .at(span)
        .with_label(v.kind.to_string());
    match help {
        Some(h) => diagnostic.with_help(h),
        None => diagnostic,
    }
}

fn locate_violation(source: &str, v: &ConventionViolation) -> Option<SourceSpan> {
    match &v.location {
        ViolationLocation::Map => SourceSpan::find(source, "<map", 0),
        ViolationLocation::Layer(name) => layer_span(source, name),
        ViolationLocation::Tileset(name) => {
            SourceSpan::find(source, &format!("name=\"{name}\""), 0)
                .or_else(|| SourceSpan::find(source, "<tileset", 0))
        }
        ViolationLocation::Tile { .. } => SourceSpan::find(source, "<tileset", 0),
        ViolationLocation::Object { id, .. } => {
            let key = match &v.kind {
                ViolationKind::WrongPropertyType { key, .. }
                | ViolationKind::InvalidPropertyValue { key, .. } => Some(key.as_str()),
                _ => None,
            };
            key.and_then(|k| property_span(source, *id, k))
                .or_else(|| object_span(source, *id))
        }
    }
}

/// Byte offset of `<object id="<id>"` in a `.tmx` source.
#[must_use]
pub fn object_offset(source: &str, id: u32) -> Option<usize> {
    find_from(source, &format!("<object id=\"{id}\""), 0)
}

/// The span of the opening `<object id="<id>"` tag.
#[must_use]
pub fn object_span(source: &str, id: u32) -> Option<SourceSpan> {
    let needle = format!("<object id=\"{id}\"");
    SourceSpan::find(source, &needle, 0)
}

/// The span of the `value="…"` of property `key` of object `id` (the property name when it
/// has no `value` attribute, the object tag when the property is missing).
#[must_use]
pub fn property_span(source: &str, id: u32, key: &str) -> Option<SourceSpan> {
    let start = object_offset(source, id)?;
    let end = find_from(source, "</object>", start).unwrap_or(source.len());
    let name = format!("<property name=\"{key}\"");
    let Some(at) = find_from(source, &name, start).filter(|&at| at < end) else {
        return object_span(source, id);
    };
    let tag_end = find_from(source, ">", at).unwrap_or(source.len());
    match find_from(source, "value=\"", at).filter(|&v| v < tag_end) {
        Some(v) => {
            let value_start = v + "value=\"".len();
            let value_end = find_from(source, "\"", value_start).unwrap_or(value_start);
            Some(SourceSpan::from_byte_range(source, value_start..value_end))
        }
        None => Some(SourceSpan::from_byte_range(source, at..at + name.len())),
    }
}

/// The span of a layer's or object group's `name="<name>"`.
#[must_use]
pub fn layer_span(source: &str, name: &str) -> Option<SourceSpan> {
    let needle = format!("name=\"{name}\"");
    let mut from = 0;
    while let Some(at) = find_from(source, &needle, from) {
        let tag = source[..at].rfind('<').map_or("", |lt| &source[lt..at]);
        if tag.starts_with("<layer")
            || tag.starts_with("<objectgroup")
            || tag.starts_with("<imagelayer")
            || tag.starts_with("<group")
        {
            return Some(SourceSpan::from_byte_range(source, at..at + needle.len()));
        }
        from = at + needle.len();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const TMX: &str = r#"<map version="1.10" width="4" height="4">
 <layer id="1" name="ground" width="4" height="4"/>
 <objectgroup id="2" name="objects">
  <object id="1" name="old_man" type="npc" x="16" y="16" width="16" height="16">
   <properties>
    <property name="script" value="maps/town.lua:old_man"/>
   </properties>
  </object>
  <object id="2" type="warp" x="0" y="0"/>
 </objectgroup>
</map>
"#;

    #[test]
    fn locators_find_objects_properties_and_layers() {
        assert_eq!(
            property_span(TMX, 1, "script"),
            Some(SourceSpan::new(6, 36, 21))
        );
        // A missing property points at its object.
        assert_eq!(
            property_span(TMX, 2, "to_map"),
            Some(SourceSpan::new(9, 3, 14))
        );
        assert_eq!(property_span(TMX, 9, "x"), None);
        assert_eq!(layer_span(TMX, "objects"), Some(SourceSpan::new(3, 22, 14)));
        assert_eq!(
            layer_span(TMX, "old_man"),
            None,
            "object names are not layers"
        );
    }

    #[test]
    fn violations_become_located_diagnostics() {
        let err = MapError::Conventions {
            path: "maps/town.tmx".into(),
            violations: vec![
                ConventionViolation {
                    location: ViolationLocation::Map,
                    kind: ViolationKind::MissingLayer("ground"),
                },
                ConventionViolation {
                    location: ViolationLocation::Object {
                        layer: "objects".into(),
                        id: 2,
                        name: String::new(),
                    },
                    kind: ViolationKind::MissingProperty("to_map"),
                },
            ],
        };
        let diags = map_error_diagnostics(&err, "maps/town.tmx", Some(TMX));
        assert_eq!(diags.len(), 2);
        assert_eq!(diags[0].code, CODE_CONVENTION);
        assert_eq!(diags[0].span, Some(SourceSpan::new(1, 1, 4)));
        assert_eq!(diags[1].span, Some(SourceSpan::new(9, 3, 14)));
        assert_eq!(
            diags[1].message,
            "object #2 in layer `objects`: missing required property `to_map`"
        );
        let parse = MapError::Parse {
            path: "maps/a.tmx".into(),
            message: "bad xml".into(),
        };
        assert_eq!(
            map_error_diagnostics(&parse, "maps/a.tmx", None)[0].code,
            CODE_PARSE
        );
    }
}
