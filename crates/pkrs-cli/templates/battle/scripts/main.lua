-- scripts/main.lua runs once when the game starts. The game opens straight into a trainer
-- battle; change `data/` to try other creatures, moves and trainers.
local arena_battle = require("arena_battle")

events.on("game_start", function()
  arena_battle()
end)
