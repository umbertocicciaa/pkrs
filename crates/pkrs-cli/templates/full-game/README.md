# {{name}}

A small creature-collecting game made with pkrs: a home town, a lab where you pick your
partner, a route with tall grass and a rival to beat.

- `pkrs run` plays the game from these files (dev saves go to `saves/`); `pkrs check`
  validates everything (`--plugins` also loads the plugin and prints its hooks).
- What happens where:
  - `maps/town.tmx` + `scripts/maps/town.lua`: Mom, a fisher, and a gate that keeps you in
    town until you have a partner.
  - `maps/lab.tmx` + `scripts/maps/lab.lua`: starter selection (`ui.choose`, `ui.yes_no`,
    `game.give_creature`); the rival picks too.
  - `maps/route.tmx` + `scripts/maps/route.lua`: wild creatures in the tall grass (the
    `encounters` layer and `data/encounters/route.ron`) and the rival's trainer battle
    (`battle.start_trainer`).
  - `scripts/main.lua` runs when the game starts.
- Content lives in `data/`: `species/` (three creatures), `moves/` (eight moves),
  `types.ron` (the type chart), `items/`, `encounters/` and `trainers/`. Your party starts
  empty (`data/trainers/player.ron`); the lab fills it. A file holds one record or a list.
- Every text is a `text:` key from `data/text/en/`. Add `data/text/<lang>/` and list the
  language in `pkrs.toml [locale]` to translate the game.
- The menu button opens the field menu: your party, your bag and saving.
- `plugins/resonance/` is an example plugin that adds a battle action; see its README.
- `tests/*.lua` play the game with scripted input, for example
  `pkrs-runtime --project . --script tests/rival_battle.lua`.
- Every asset folder has a `LICENSES.md`; list the author and license of anything you add.
