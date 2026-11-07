-- scripts/maps/town.lua: the rival challenges the player to a trainer battle and reacts to
-- the result the battle hands back (design §7.2, §9.3).
function rival(npc)
  npc:face_player()
  if game.flag("beat_rival") then
    ui.say("You're too strong!")
    return
  end
  ui.say("Let's battle!")
  local result = battle.start_trainer("rival")
  game.set_var("battle_outcome", result.outcome)
  game.set_var("battle_turns", result.turns)
  if result.outcome == "win" then
    game.set_flag("beat_rival", true)
    ui.say("You beat me!")
  else
    ui.say("Better luck next time.")
  end
end
