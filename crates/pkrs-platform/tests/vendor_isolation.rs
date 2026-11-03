//! §4.5: each vendor crate is imported only by its adapter module.

// Test-only helpers outside `#[test]` fns are not covered by clippy.toml's test exemption.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

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

/// Source lines (comments skipped) outside `adapter` that use the `vendor` crate path.
fn offending_lines(vendor: &str, adapter: &str) -> Vec<String> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);

    let needle = format!("{vendor}::");
    let mut offenders = Vec::new();
    for file in files {
        let rel = file.strip_prefix(&src).unwrap();
        if rel == Path::new(adapter) {
            continue;
        }
        for (n, line) in fs::read_to_string(&file).unwrap().lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            let uses_vendor = code.match_indices(&needle).any(|(i, _)| {
                // `self::macroquad::X` re-exports the adapter module, not the vendor crate.
                !code[..i].ends_with("self::")
            });
            if uses_vendor || code.contains(&format!("extern crate {vendor}")) {
                offenders.push(format!("{}:{}: {}", rel.display(), n + 1, code));
            }
        }
    }
    offenders
}

#[test]
fn macroquad_only_in_its_adapter() {
    let offenders = offending_lines("macroquad", "backend/macroquad.rs");
    assert!(
        offenders.is_empty(),
        "macroquad used outside its adapter:\n{offenders:#?}"
    );
}

#[test]
fn directories_only_in_its_adapter() {
    let offenders = offending_lines("directories", "backend/directories.rs");
    assert!(
        offenders.is_empty(),
        "directories used outside its adapter:\n{offenders:#?}"
    );
}
