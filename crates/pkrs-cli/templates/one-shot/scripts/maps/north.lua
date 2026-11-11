-- scripts/maps/north.lua: the keeper's cutscene. He asks a yes/no question; saying yes walks
-- him to the cliff edge and moves the story to the ending map.

function keeper(npc)
  npc:face_player()
  if not game.flag("met_guide") then
    ui.say("Hm? Say hello to the guide on the village green first.")
    return
  end
  ui.say("You came! The sun is about to set.")
  if not ui.yes_no("Will you watch it with me?") then
    ui.say("Come back when you are ready. I will wait.")
    return
  end
  ui.say("Then come. The best seat is by the edge.")
  npc:move({ "left", "left", "up" })
  npc:face("up")
  player.face("up")
  wait(30)
  map.warp("ending", 7, 6, "up")
end
