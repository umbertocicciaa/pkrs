events.on("game_start", function()
  if game.flag("started") then
    ui.say("text:intro.again")
end)
