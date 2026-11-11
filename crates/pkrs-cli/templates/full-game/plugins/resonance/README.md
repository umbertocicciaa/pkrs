# Resonance (example plugin)

Once per battle, a creature can **Resonate** for 3 turns: its moves of its primary type deal
1.5x damage, and it glows. The whole gimmick is plugin code (docs/design.md §9.5):

- `plugin.toml`: the manifest. The game enables it in `pkrs.toml` (`[plugins] enabled`).
- `init.lua`: a `RESONATE` entry in the fight menu (`on_action_menu` + `on_custom_action`), a
  damage modifier (`on_damage_calc`) and a 3-turn countdown (`on_end_of_turn`).
- `data/text/en/resonance.ron`: the plugin's texts.
- `assets/aura/glow.png`: the glow, played as the `resonance:aura` battle effect.

Disable it by removing `"resonance"` from `[plugins] enabled`; `pkrs check --plugins` prints
the hooks it registers.
