//! §4.5: vendor crates are imported only from their adapter module (workspace-wide).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

const ROWS: &[(&str, &str, &str)] = &[
    ("pkrs-platform", "macroquad", "backend/macroquad.rs"),
    ("pkrs-platform", "directories", "backend/directories.rs"),
    ("pkrs-render", "macroquad", "backend/macroquad.rs"),
    ("pkrs-world", "tiled", "importer/tiled.rs"),
    ("pkrs-core", "rand_pcg", "rng/adapter.rs"),
    ("pkrs-core", "toml", "manifest/toml_codec.rs"),
    ("pkrs-core", "semver", "version.rs"),
    ("pkrs-data", "ron", "codec.rs"),
    ("pkrs-script", "mlua", "lua/"),
];

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn line_uses_vendor(code: &str, vendor: &str) -> bool {
    if code.contains(&format!("extern crate {vendor}")) {
        return true;
    }
    let needle = format!("{vendor}::");
    if code
        .match_indices(&needle)
        .any(|(i, _)| !code[..i].ends_with("self::"))
    {
        return true;
    }
    if code.starts_with("use ")
        && code.split_whitespace().any(|token| {
            token == vendor
                || token.starts_with(&format!("{vendor}::"))
                || token.starts_with(&format!("::{vendor}"))
        })
    {
        return true;
    }
    false
}

fn offending_lines(crate_src: &Path, vendor: &str, adapter: &str) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(crate_src, &mut files);

    let mut offenders = Vec::new();
    for file in files {
        let rel = file.strip_prefix(crate_src).unwrap();
        // An adapter ending in `/` is a whole module folder.
        let inside_adapter = match adapter.strip_suffix('/') {
            Some(dir) => rel.starts_with(dir),
            None => rel == Path::new(adapter),
        };
        if inside_adapter {
            continue;
        }
        for (n, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            if line_uses_vendor(code, vendor) {
                offenders.push(format!("{}:{}: {}", rel.display(), n + 1, code));
            }
        }
    }
    offenders
}

#[test]
fn vendor_crates_only_in_adapters() {
    let root = workspace_root();
    for (krate, vendor, adapter) in ROWS {
        let src = root.join("crates").join(krate).join("src");
        let offenders = offending_lines(&src, vendor, adapter);
        assert!(
            offenders.is_empty(),
            "{krate}: {vendor} used outside {adapter}:\n{offenders:#?}"
        );
    }
}
