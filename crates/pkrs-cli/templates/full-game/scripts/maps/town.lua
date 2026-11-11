-- scripts/maps/town.lua: the home town. Mom gives potions once you have a partner, the
-- fisher rewards beating the rival, and the gate keeps you in town until the lab gave you a
-- starter.

function mom(npc)
  npc:face_player()
  if not game.flag("got_starter") then
    ui.say("text:town.mom_go_to_lab")
  elseif not game.flag("mom_potions") then
    ui.say("text:town.mom_potions")
    game.give_item("potion", 2)
    game.set_flag("mom_potions")
  else
    ui.say("text:town.mom_take_care")
  end
end

function fisher(npc)
  npc:face_player()
  if game.flag("beat_rival") and not game.flag("fisher_gift") then
    ui.say("text:town.fisher_gift")
    game.give_item("super_potion", 1)
    game.set_flag("fisher_gift")
  else
    ui.say("text:town.fisher")
  end
end

-- The `route_gate` trigger covers the north exit (maps/town.tmx).
function route_gate(e)
  if game.flag("got_starter") then
    return
  end
  ui.say("text:town.gate")
  map.warp("town", e.x, e.y + 1, "down")
end
