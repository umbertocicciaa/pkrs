-- The keeper the map's npc names is missing, and `intro` never records progress.
function intro()
  ui.say("text:sign.welcome")
end

events.on("tick", function()
  -- runs every tick
end)
