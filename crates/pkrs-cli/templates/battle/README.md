# {{name}}

A pkrs battle playground: the game opens straight into a trainer battle.

- `pkrs run` starts the game. The battle against the rival begins right away; after it you
  can ask for a rematch, or walk up to the rival (blond, green shirt, three tiles above
  the start) and talk to them for another battle. NPCs use `sprites/<sprite>.png` from
  their `sprite` property in the map.
- Every battle starts with full HP and PP.
- Edit the content in `data/`:
  - `data/trainers/player.ron`: your party,
  - `data/trainers/rival.ron`: the opponent,
  - `data/species/`, `data/moves/`, `data/types.ron`: creatures, moves and the type chart.
- `scripts/arena_battle.lua` starts the battles; `scripts/main.lua` runs it when the game
  starts.
- `tests/battle.lua` plays one battle with scripted input:
  `pkrs-runtime --project . --script tests/battle.lua`.
- `pkrs.toml` is the project manifest.
