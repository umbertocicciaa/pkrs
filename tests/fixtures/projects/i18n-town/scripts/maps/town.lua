-- scripts/maps/town.lua: the guide greets the player in the active language and offers to
-- switch it (the options API, `game.set_language`).
function guide(npc)
  npc:face_player()
  ui.say("text:guide.hello", { town = "Borgo Lingua" })
  local other = game.language() == "it" and "en" or "it"
  if ui.yes_no("text:guide.switch", { lang = game.text("text:lang." .. other) }) then
    game.set_language(other)
    game.set_var("switches", (game.var("switches") or 0) + 1)
  end
  ui.say("text:guide.bye", { 0 + (game.var("switches") or 0) })
end
