-- Game script: listeners that live for the whole game (unlike per-map scripts).
local function note(what)
  game.set_var("order", (game.var("order") or "") .. what .. ";")
end

events.on("game_start", function() note("game:start") end)
events.on("map_enter", function(e) note("game:enter:" .. e.map) end)
events.on("map_exit", function(e) note("game:exit:" .. e.map) end)
