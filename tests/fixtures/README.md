# Test fixtures

Shared by integration tests across crates (runtime, CLI, core).

- `projects/<name>/` — sample game projects. Each one has a `pkrs.toml`; the other folders are optional (design §2.1).
  - `minimal`: only `[game]`, `[engine]` and `[start]`, so every other value is a default.
  - `full`: every `pkrs.toml` table from design §2.2, including `[input]`.
  - `story-only`: a one-shot story with `features.battles = false`.
  - `invalid`: valid TOML that fails validation in several fields.
  - `broken-toml`: a TOML syntax error.
  - `scenario-town`: a playable town (NPC with a dialogue and a choice, a sign, a warp into a
    house) whose `tests/*.lua` are scripted-input scenarios (design §15.2). Every fixture
    project with a `tests/` folder is run by `crates/pkrs-runtime/tests/scenarios.rs`;
    `tests/smoke.lua` is the CI smoke run (§14.4).
