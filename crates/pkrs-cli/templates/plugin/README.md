# {{name}}

A pkrs plugin (`{{id}}`, docs/design.md §9.4).

- `plugin.toml` is the plugin manifest: id, version, scripting API level, dependencies and
  load-order hints.
- `init.lua` runs once at start-up and registers battle hooks and world event handlers.
- Enable it by listing `"{{id}}"` in `[plugins].enabled` of the project's `pkrs.toml`.
- `pkrs check --plugins` loads the enabled plugins and prints the load order and hook table.
