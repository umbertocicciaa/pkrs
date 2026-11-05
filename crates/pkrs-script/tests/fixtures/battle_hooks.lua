-- Fixture: doubles the player's damage, cancels the opponent's first move,
-- announces each turn and pays out at the end.

battle.on("on_damage_calc", function(ctx, user, move, target, dmg)
  if user and user.is_player then
    dmg.modifiers:push("fixture_boost", 2)
  end
  return dmg
end, { priority = 10 })

battle.on("on_before_move", function(ctx, user, move, target)
  if not user.is_player and ctx.turn == 1 then
    return { cancel = true, message = "text:fixture.flinch" }
  end
end)

battle.on("on_turn_start", function(ctx)
  ctx.message("text:fixture.turn_" .. ctx.turn)
end)

battle.on("on_battle_end", function(ctx, result)
  result.money = 77
  return result
end)
