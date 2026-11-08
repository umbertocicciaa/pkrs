function twins(npc)
  npc:face_player()
  if game.flag("beat_twins") then
    ui.say("We've been training!")
    return
  end
  ui.say("Let's battle!")
  local result = battle.start_trainer("twins")
  game.set_var("battle_outcome", result.outcome)
  game.set_var("battle_turns", result.turns)
  if result.outcome == "win" then
    game.set_flag("beat_twins", true)
    ui.say("You beat the twins!")
  else
    ui.say("We'll get you next time.")
  end
end
