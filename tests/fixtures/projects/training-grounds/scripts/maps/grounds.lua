-- scripts/maps/grounds.lua: the coach battles the player every time they talk, and records
-- what each battle handed back (design §7.2, §7.4).
function coach(npc)
  npc:face_player()
  ui.say("Show me your training!")
  local result = battle.start_trainer("coach")
  game.set_var("battle_outcome", result.outcome)
  game.set_var("battle_exp", result.exp)
  if result.outcome == "win" then
    game.set_var("wins", (game.var("wins") or 0) + 1)
    ui.say("Well fought!")
  else
    ui.say("Keep training.")
  end
end
