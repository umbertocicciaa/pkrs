//! Material descriptors (§5.6): every `assets/materials/**/*.ron` is checked by
//! [`pkrs_render::check_material`] (schema, shader, capability profile), and map
//! references to materials must name one of them.

use std::collections::BTreeSet;

use pkrs_core::Diagnostics;
use pkrs_render::{MATERIALS_DIR, check_material};

use crate::host::ScriptFiles;

/// Checks every material descriptor and returns the ids that exist (`water`, `fx/glow`).
pub(super) fn check_all(files: &dyn ScriptFiles, out: &mut Diagnostics) -> BTreeSet<String> {
    let mut paths = Vec::new();
    walk(files, MATERIALS_DIR, 0, &mut paths);
    paths.sort();
    let read = |path: &str| files.read_script(path).ok();
    let mut ids = BTreeSet::new();
    for path in paths {
        let Some(id) = path
            .strip_prefix(MATERIALS_DIR)
            .and_then(|p| p.strip_prefix('/'))
            .and_then(|p| p.strip_suffix(".ron"))
        else {
            continue;
        };
        ids.insert(id.to_owned());
        let Ok(source) = files.read_script(&path) else {
            continue;
        };
        out.extend(check_material(&path, &source, &read));
    }
    ids
}

fn walk(files: &dyn ScriptFiles, dir: &str, depth: usize, out: &mut Vec<String>) {
    if depth > 8 {
        return;
    }
    for name in files.list_files(dir) {
        if name.ends_with(".ron") {
            out.push(format!("{dir}/{name}"));
        }
    }
    for sub in files.list_dirs(dir) {
        walk(files, &format!("{dir}/{sub}"), depth + 1, out);
    }
}
