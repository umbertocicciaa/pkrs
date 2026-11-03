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
