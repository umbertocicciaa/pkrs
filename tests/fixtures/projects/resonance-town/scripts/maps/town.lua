-- scripts/maps/town.lua: start a wild battle so the Resonance plugin can add its menu entry.
function sage(npc)
  npc:face_player()
  if game.flag("tried_resonance") then
    ui.say("The aura fades.")
    return
  end
  ui.say("Focus your type!")
  local result = battle.start_wild("leafkit", 10)
  game.set_flag("tried_resonance", true)
  game.set_var("battle_outcome", result.outcome)
  if result.outcome == "win" then
    ui.say("Well done!")
  elseif result.outcome == "fled" then
    ui.say("You ran away.")
  else
    ui.say("Try again.")
  end
end
