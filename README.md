# pkrs
2D RPG SDK written in Rust

## Building and testing

pkrs is a Cargo workspace of 13 crates under [`crates/`](crates/), laid out as described in
[docs/design.md §4.3](docs/design.md#43-crate-layout). The Rust toolchain is pinned in
[`rust-toolchain.toml`](rust-toolchain.toml); `rustup` installs it automatically on first use.

```sh
cargo build --workspace                                  # build every crate
cargo test --workspace                                   # unit + integration tests
cargo fmt --all --check                                  # formatting
cargo clippy --workspace --all-targets -- -D warnings    # lints (unwrap/expect denied)
```

Binaries:

```sh
cargo run -p pkrs-cli        # the `pkrs` command-line tool
cargo run -p pkrs-runtime    # the game player
```

Logging goes to stderr through `tracing`; set `RUST_LOG` (e.g. `RUST_LOG=debug`) to change the level.

The internal crate dependency graph is checked by
[`crates/pkrs-cli/tests/crate_graph.rs`](crates/pkrs-cli/tests/crate_graph.rs): adding a
`pkrs-*` dependency that is not in §4.3 fails `cargo test`.

## Continuous integration

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every pull request and every push
to `main` ([docs/design.md §14.4](docs/design.md#144-ci-matrix-github-actions), §15.3):

- **`check (<os>)`** on `ubuntu-latest`, `windows-latest` and `macos-latest`: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `cargo build --workspace`. Linux installs the X11/GL/ALSA/udev dev headers first.
- **`cargo-deny`**: `cargo deny check advisories bans licenses sources` with [`deny.toml`](deny.toml).
  Its `[bans]` section enforces §4.5 Rule 2 with `wrappers`: each vendor crate may only be a
  direct dependency of its owning crate (e.g. adding `macroquad` to `pkrs-world` fails
  `cargo deny check bans`). Update `deny.toml` whenever the §4.5 dependency map changes.

Run the same checks locally with `cargo install cargo-deny --locked && cargo deny check`.

### Required status checks

Branch protection on `main` should require these checks (Settings → Branches → `main` →
*Require status checks to pass before merging*):

| Check name |
|---|
| `check (ubuntu-latest)` |
| `check (windows-latest)` |
| `check (macos-latest)` |
| `cargo-deny` |
