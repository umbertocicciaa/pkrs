-- scripts/maps/arena.lua: talking to the rival starts another battle.
local arena_battle = require("arena_battle")

function rival(npc)
  npc:face_player()
  ui.say("Let's battle!")
  arena_battle()
end
