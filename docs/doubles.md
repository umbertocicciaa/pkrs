# Doubles battles (default plugin)

Double battles use the core **N-active-slot** model (`BattleConfig.side_size`, `TrainerDef.active_slots`) without changing singles defaults.

## Enabling doubles

1. Add the `doubles` plugin to `[plugins].enabled` in `pkrs.toml`.
2. Set `active_slots: 2` on trainers that send two creatures (in game or plugin `data/trainers/*.ron`).
3. Ensure the player's `data/trainers/player.ron` has at least two party members and `active_slots: 2` when the player fights in doubles.

Wild battles stay 1v1 until a future format hook; trainer battles use `max(player.active_slots, opponent.active_slots)` as `side_size`.

## Spread moves

Moves may set `targeting: all_foes` in RON (`MoveTargetScope::AllFoes`). The battle core resolves every non-fainted foe; the UI skips the target menu.

## UI

With `side_size = 2`, the battle screen collects one action per active player slot each turn. Single-target moves open the **target** menu when both foes are active.

## Fixture

`tests/fixtures/projects/doubles-arena/` runs `tests/doubles_battle.lua` end-to-end.
