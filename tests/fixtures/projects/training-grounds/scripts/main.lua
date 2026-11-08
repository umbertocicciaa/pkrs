-- scripts/main.lua: training is worth double (design §7.4: `on_battle_end` may change the
-- rewards). The engine fills `result.exp` with the default exp of the defeated creatures.
battle.on("on_battle_end", function(ctx, result)
  result.exp = result.exp * 2
  return result
end)
