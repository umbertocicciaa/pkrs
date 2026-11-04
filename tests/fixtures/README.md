# Test fixtures

Shared by integration tests across crates (runtime, CLI, core).

- `projects/<name>/` — sample game projects. Each one has a `pkrs.toml`; the other folders are optional (design §2.1).
  - `minimal`: only `[game]`, `[engine]` and `[start]`, so every other value is a default.
  - `full`: every `pkrs.toml` table from design §2.2, including `[input]`.
  - `story-only`: a one-shot story with `features.battles = false`.
  - `invalid`: valid TOML that fails validation in several fields.
  - `broken-toml`: a TOML syntax error.
