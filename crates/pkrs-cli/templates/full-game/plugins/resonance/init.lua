-- plugins/resonance/init.lua: the Resonance gimmick (docs/design.md §9.5), written entirely as a
-- plugin. Once per battle, a creature can Resonate for 3 turns: its moves of its primary type
-- deal 1.5x damage, and it glows (`assets/aura/glow.png`).
local P = plugin.define()

local DURATION = 3

battle.on("on_action_menu", function(ctx, side, options)
  if side.is_player and not ctx.side_state(side, "resonance_used") then
    table.insert(options, { id = "resonance:resonate", label = "text:resonance.menu" })
  end
  return options
end)

battle.on("on_custom_action", function(ctx, id, payload)
  if id ~= "resonance:resonate" then return end
  local mon = payload.creature
  ctx.set_side_state(mon.side, "resonance_used", true)
  ctx.set_volatile(mon, "resonating", { turns = DURATION })
  ctx.message("text:resonance.start", { name = mon.name })
  ctx.play_fx("resonance:aura", mon)            -- shown on this creature until cleared
end)

battle.on("on_damage_calc", function(ctx, user, move, target, dmg)
  local r = ctx.volatile(user, "resonating")
  if r and move.type == user.types[1] then
    dmg.modifiers:push("resonance", 1.5)
  end
  return dmg
end, { priority = 10 })

battle.on("on_end_of_turn", function(ctx)
  for _, mon in ipairs(ctx.active_creatures()) do
    local r = ctx.volatile(mon, "resonating")
    if r then
      r.turns = r.turns - 1
      if r.turns <= 0 then
        ctx.clear_volatile(mon, "resonating")
        ctx.message("text:resonance.end", { name = mon.name })
        ctx.clear_fx(mon)
      else
        ctx.set_volatile(mon, "resonating", r)
      end
    end
  end
end)

return P
