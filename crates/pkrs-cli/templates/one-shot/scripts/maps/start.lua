-- scripts/maps/start.lua: the village green. Talking to the guide opens a choice menu; the
-- answer only changes what the guide says, the story goes on either way.

function guide(npc)
  npc:face_player()
  if game.flag("met_guide") then
    ui.say("The keeper is waiting on the northern cliff. Take the gap in the trees!")
    return
  end
  ui.say("Oh! A new face in Willowmere.")
  local topic = ui.choose({ "Who are you?", "What should I do?", "Nothing, thanks." })
  if topic == 1 then
    ui.say("I look after the village green. The keeper looks after the sea.")
  elseif topic == 2 then
    ui.say("Climb the path to the north, past the gap in the trees.")
  else
    ui.say("Take your time, then.")
  end
  ui.say("The keeper is waiting on the northern cliff. Don't keep him waiting!")
  game.set_flag("met_guide")
end
