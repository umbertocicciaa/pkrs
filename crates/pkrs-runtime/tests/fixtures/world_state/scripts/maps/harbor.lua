-- scripts/maps/harbor.lua: persistent map state fixture (§6.5).
function sailor(npc)
  npc:face_player()
  map.set_var("talks", (map.var("talks") or 0) + 1)
  if map.var("talks") == 1 then
    ui.say("text:sailor.hello")
    npc:move({ "left" })
    map.npc("ghost"):hide()
    game.set_flag("sailor_met", true)
  else
    ui.say("text:sailor.again")
  end
end

function dock_hint(e)
  map.set_var("hints", (map.var("hints") or 0) + 1)
end

events.on("map_enter", function(e)
  map.set_var("visits", (map.var("visits") or 0) + 1)
end)

events.on("interact", function(e)
  if e.kind == "item" then
    game.set_var("picked", e.item .. ":" .. e.id .. ":" .. e.count)
  end
end)
