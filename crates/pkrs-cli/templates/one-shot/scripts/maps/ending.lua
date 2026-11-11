-- scripts/maps/ending.lua: the ending. Entering the map plays the closing text once, then
-- the player may stay and look at the stars.

events.on("map_enter", function(e)
  if e.map ~= "ending" or game.flag("story_complete") then
    return
  end
  player.lock()
  wait(40)
  ui.say("The sun sinks into the sea, and the first stars come out.")
  ui.say("Keeper: Thank you for staying. Every story needs a good ending.")
  ui.say("THE END")
  ui.say("Thanks for playing {{name}}!")
  game.set_flag("story_complete")
  player.unlock()
end)

function keeper(npc)
  ui.say("Look, a shooting star!")
end
