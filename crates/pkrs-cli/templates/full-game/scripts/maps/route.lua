-- scripts/maps/route.lua: the rival waits at the end of the route. Stepping onto the
-- `rival_challenge` trigger starts a trainer battle against the rival's starter
-- (data/trainers/rival_<species>.ron); losing lets you try again.

function rival_challenge(e)
  if game.flag("beat_rival") then
    return
  end
  local rival = map.npc("rival")
  if rival then
    rival:face_player()
  end
  ui.say("text:route.rival_challenge")
  local trainer = "rival_" .. (game.var("rival_starter") or "leafkit")
  local result = battle.start_trainer(trainer)
  if result == nil then
    return -- the battle could not be built; the reason is in the log
  end
  if result.outcome == "win" then
    game.set_flag("beat_rival")
    ui.say("text:route.rival_lost")
  else
    ui.say("text:route.rival_won")
  end
end

function rival(npc)
  npc:face_player()
  if game.flag("beat_rival") then
    ui.say("text:route.rival_after")
  else
    ui.say("text:route.rival_ready")
  end
end
