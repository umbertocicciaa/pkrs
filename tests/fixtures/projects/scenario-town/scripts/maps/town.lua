-- scripts/maps/town.lua: the guide greets the player, asks where they are headed and
-- remembers the answer.
function guide(npc)
  npc:face_player()
  if game.flag("met_guide") then
    ui.say("Safe travels!")
    return
  end
  ui.say("Welcome, traveller!")
  local pick = ui.choose({ "North", "South", "Nowhere" })
  game.set_var("destination", pick)
  if pick == 1 then
    ui.say("North it is.")
  elseif pick == 2 then
    ui.say("South it is.")
  else
    ui.say("Suit yourself.")
  end
  game.set_flag("met_guide", true)
end

events.on("map_enter", function(e)
  game.set_var("visits_" .. e.map, (game.var("visits_" .. e.map) or 0) + 1)
end)
