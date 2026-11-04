//! `pkrs-battle` must not depend on rendering, scripting or I/O crates (issue #32).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

const FORBIDDEN: &[&str] = &[
    "macroquad",
    "mlua",
    "pkrs-render",
    "pkrs-script",
    "pkrs-platform",
    "pkrs-assets",
];

#[test]
fn dependency_tree_has_no_render_script_or_io() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = Command::new(cargo)
        .args(["tree", "-p", "pkrs-battle", "--prefix", "none"])
        .output()
        .expect("cargo tree");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let tree = String::from_utf8_lossy(&output.stdout);
    for name in FORBIDDEN {
        assert!(
            !tree.lines().any(|line| line.contains(name)),
            "pkrs-battle must not depend on {name}:\n{tree}"
        );
    }
    assert!(tree.contains("pkrs-core"));
    assert!(tree.contains("pkrs-data"));
}
