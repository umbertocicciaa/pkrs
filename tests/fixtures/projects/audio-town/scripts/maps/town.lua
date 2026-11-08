-- scripts/maps/town.lua: the rival plays a sound, challenges the player to a trainer battle
-- and reacts to the result (design §7.2, §9.3, §11).
function rival(npc)
  npc:face_player()
  if game.flag("beat_rival") then
    ui.say("You're too strong!")
    return
  end
  audio.play_sfx("exclaim")
  ui.say("Let's battle!")
  local result = battle.start_trainer("rival")
  game.set_var("battle_outcome", result.outcome)
  if result.outcome == "win" then
    game.set_flag("beat_rival", true)
    ui.say("You beat me!")
  else
    ui.say("Better luck next time.")
  end
end
