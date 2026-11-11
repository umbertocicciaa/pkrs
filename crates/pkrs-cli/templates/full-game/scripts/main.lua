-- scripts/main.lua runs once when the game starts, before the first map's events.
-- Every text is a `text:` key from data/text/<lang>/*.ron (docs/design.md §5.5).

events.on("game_start", function()
  ui.say("text:intro.welcome")
end)
