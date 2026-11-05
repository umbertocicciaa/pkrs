//! Workspace-level check: the internal crate dependency graph must match
//! `docs/design.md` §4.3 exactly (no missing and no extra edges).

// Test-only helpers outside `#[test]` fns are not covered by clippy.toml's test exemption.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

/// Allowed `pkrs-*` → `pkrs-*` edges (normal and build dependencies), per §4.3.
const ALLOWED: &[(&str, &[&str])] = &[
    ("pkrs-core", &[]),
    ("pkrs-platform", &["pkrs-core"]),
    ("pkrs-assets", &["pkrs-core"]),
    ("pkrs-data", &["pkrs-core"]),
    (
        "pkrs-render",
        &["pkrs-core", "pkrs-platform", "pkrs-assets"],
    ),
    ("pkrs-audio", &["pkrs-assets"]),
    ("pkrs-world", &["pkrs-data", "pkrs-assets"]),
    ("pkrs-battle", &["pkrs-core", "pkrs-data"]),
    ("pkrs-save", &["pkrs-core", "pkrs-data"]),
    ("pkrs-ui", &["pkrs-render", "pkrs-platform"]),
    (
        "pkrs-script",
        &[
            "pkrs-world",
            "pkrs-battle",
            "pkrs-data",
            "pkrs-save",
            "pkrs-ui",
            "pkrs-audio",
        ],
    ),
    (
        "pkrs-runtime",
        &[
            "pkrs-core",
            "pkrs-platform",
            "pkrs-script",
            "pkrs-render",
            "pkrs-audio",
            "pkrs-ui",
            "pkrs-save",
            "pkrs-world",
        ],
    ),
    (
        "pkrs-cli",
        &["pkrs-data", "pkrs-assets", "pkrs-world", "pkrs-script"],
    ),
];

type Graph = BTreeMap<String, BTreeSet<String>>;

fn expected_graph() -> Graph {
    ALLOWED
        .iter()
        .map(|(krate, deps)| {
            (
                krate.to_string(),
                deps.iter().map(|d| d.to_string()).collect(),
            )
        })
        .collect()
}

fn actual_graph() -> Graph {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--offline",
        ])
        .current_dir(&workspace_root)
        .output()
        .expect("failed to run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("cargo metadata emitted invalid JSON");

    let packages = metadata["packages"].as_array().expect("`packages` array");
    packages
        .iter()
        .map(|pkg| {
            let name = pkg["name"].as_str().expect("package name").to_string();
            let deps = pkg["dependencies"]
                .as_array()
                .expect("`dependencies` array")
                .iter()
                // `kind` is null for normal deps, "build" or "dev" otherwise.
                .filter(|dep| dep["kind"].as_str() != Some("dev"))
                .filter_map(|dep| dep["name"].as_str())
                .filter(|dep| dep.starts_with("pkrs-"))
                .map(str::to_string)
                .collect();
            (name, deps)
        })
        .collect()
}

#[test]
fn workspace_contains_exactly_the_designed_crates() {
    let expected: BTreeSet<_> = expected_graph().into_keys().collect();
    let actual: BTreeSet<_> = actual_graph().into_keys().collect();
    assert_eq!(actual, expected, "workspace members differ from §4.3");
}

#[test]
fn crate_dependency_edges_match_design() {
    let expected = expected_graph();
    let actual = actual_graph();
    for (krate, deps) in &actual {
        let allowed = expected.get(krate).cloned().unwrap_or_default();
        let extra: Vec<_> = deps.difference(&allowed).collect();
        let missing: Vec<_> = allowed.difference(deps).collect();
        assert!(
            extra.is_empty(),
            "{krate} has edges not allowed by §4.3: {extra:?}"
        );
        assert!(
            missing.is_empty(),
            "{krate} is missing edges required by §4.3: {missing:?}"
        );
    }
}

#[test]
fn battle_stays_pure() {
    // §4.3: pkrs-battle must not reach rendering, scripting or I/O crates.
    let graph = actual_graph();
    let mut stack = vec!["pkrs-battle".to_string()];
    let mut seen = BTreeSet::new();
    while let Some(krate) = stack.pop() {
        if seen.insert(krate.clone()) {
            stack.extend(graph.get(&krate).into_iter().flatten().cloned());
        }
    }
    for forbidden in [
        "pkrs-platform",
        "pkrs-render",
        "pkrs-script",
        "pkrs-assets",
        "pkrs-save",
    ] {
        assert!(
            !seen.contains(forbidden),
            "pkrs-battle transitively depends on {forbidden}"
        );
    }
}
