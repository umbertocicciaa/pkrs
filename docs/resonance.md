# Resonance plugin walkthrough (§9.5)

The **Resonance** gimmick ships as a reference plugin under
`tests/fixtures/projects/resonance-town/plugins/resonance/`. It adds a once-per-battle
**Resonate** action, a timed volatile, a primary-type damage boost, battle text, and a glowing
aura FX—without any engine-specific Resonance code in `pkrs-battle`.

## Layout

```text
plugins/resonance/
  plugin.toml          # manifest (id, api, version)
  init.lua               # battle hooks (see design.md §9.5)
  assets/aura/           # plugin asset namespace `resonance:aura` (see LICENSE)
```

Enable it in `pkrs.toml`:

```toml
[plugins]
enabled = ["resonance"]
```

## How the hooks fit together

1. **`on_action_menu`** — appends `{ id = "resonance:resonate", label = "text:resonance.menu" }`
   when the player has not used Resonance this battle.
2. **`on_custom_action`** — marks the side, sets the `resonating` volatile (3 turns), shows
   text, and calls `ctx.play_fx("resonance:aura", mon)`.
3. **`on_damage_calc`** (priority 10) — multiplies damage by 1.5× when the user is resonating
   and the move matches their primary type.
4. **`on_end_of_turn`** — counts down `resonating`; when it runs out, clears the volatile,
   shows end text and calls `ctx.clear_fx(mon)` to remove the aura.

`ctx.play_fx(id, creature)` logs a **`BattleEvent::Fx { side, slot, id: Some(id) }`** for that
creature's own side and slot, and `ctx.clear_fx(creature)` logs the same event with `id: None`.
The battle UI maps each one to an instant **`Cue::Fx`**: the aura stays on that creature's
sprite until the plugin clears it (or the creature faints or is withdrawn). The engine does not
know which status an effect belongs to, so a plugin that shows an FX for a timed state clears
it itself when that state ends, as Resonance does in `on_end_of_turn`. Both calls raise a Lua
error for a missing or empty id or an argument that is not a creature table.

## Try it headless

```bash
cargo run -p pkrs-cli -- run --headless \
  --project tests/fixtures/projects/resonance-town \
  --script tests/resonance_battle.lua
```

## Tests

| Layer | Test |
|-------|------|
| Lua API | `pkrs-script` `resonance_plugin_runs_on_the_battle_api` |
| Aura cleared by the plugin | `pkrs-script` `resonance_plugin_clears_its_aura_when_the_volatile_ends` |
| Event log snapshot | `pkrs-script` `resonance_battle_replay_snapshot` |
| Headless scenario | `resonance-town/tests/resonance_battle.lua` via `scenarios.rs` |

Copy `plugins/resonance/` into your own project tree to experiment; run `pkrs check --plugins`
to verify load order and hook registration.
