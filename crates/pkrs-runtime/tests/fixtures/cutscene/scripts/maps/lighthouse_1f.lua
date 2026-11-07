-- scripts/maps/lighthouse_1f.lua
function old_keeper(npc)
  npc:face_player()
  if not game.flag("keeper_met") then
    ui.say("text:keeper.hello")
    local yes = ui.yes_no("text:keeper.help_me")
    if yes then
      npc:move({ "up", "up", "left" })
      wait(20)
      ui.say("text:keeper.thanks")
      game.give_item("lamp_oil", 1)
      game.set_flag("keeper_met", true)
    end
  else
    ui.say("text:keeper.again")
  end
end

-- Test hooks (not part of the §9.3 example): record what this map's handlers saw.
local function note(what)
  game.set_var("order", (game.var("order") or "") .. what .. ";")
end

events.on("map_enter", function(e) note("map:enter:" .. e.map) end)
events.on("map_exit", function(e) note("map:exit:" .. e.map) end)
events.on("step", function(e)
  game.set_var("steps", (game.var("steps") or 0) + 1)
end)
events.on("trigger_enter", function(e) note("map:trigger:" .. e.id) end)
events.on("interact", function(e) note("map:interact:" .. e.kind) end)

function stairs_hint(e)
  note("map:stairs_hint")
end
