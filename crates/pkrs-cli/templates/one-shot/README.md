# {{name}}

A one-shot story made with pkrs: a village, a guide, a keeper on the northern cliff and an
ending under the stars. No battles: `features.battles = false` in `pkrs.toml`, so the game
needs no creature data at all.

- `pkrs run` plays the game from these files (dev saves go to `saves/`).
- `pkrs check` validates the manifest, the maps and the Lua scripts.
- The story, in play order:
  - `maps/start.tmx` + `scripts/maps/start.lua`: the guide asks you a question (`ui.choose`).
  - `maps/north.tmx` + `scripts/maps/north.lua`: the keeper's cutscene (`ui.yes_no`,
    `npc:move`, `map.warp`).
  - `maps/ending.tmx` + `scripts/maps/ending.lua`: the ending, played once on `map_enter`.
  - `maps/dest.tmx`: a quiet garden south of the village.
- Edit the maps with [Tiled](https://www.mapeditor.org/). Layers follow the pkrs conventions
  (`ground`, `ground2`, `overhead`, `objects`). NPCs use `sprites/<sprite>.png` from their
  `sprite` property and call `script = "maps/<map>.lua:<function>"` when you talk to them.
- `tests/story.lua` plays the whole story with scripted input:
  `pkrs-runtime --project . --script tests/story.lua`.
- `pkrs.toml` is the project manifest. Every asset folder has a `LICENSES.md`; list the
  author and license of anything you add there.
