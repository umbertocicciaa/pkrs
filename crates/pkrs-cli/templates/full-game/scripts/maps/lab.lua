-- scripts/maps/lab.lua: starter selection. The professor offers three creatures; the one you
-- confirm joins your party with `game.give_creature`, and the rival takes the one yours is
-- strong against (you fight it on the route).

local STARTERS = { "leafkit", "emberkit", "puddlefin" }
local RIVAL_PICK = { leafkit = "puddlefin", emberkit = "leafkit", puddlefin = "emberkit" }
-- Text keys are written out in full so `pkrs check` can verify each one.
local NAMES = {
  leafkit = "text:species.leafkit",
  emberkit = "text:species.emberkit",
  puddlefin = "text:species.puddlefin",
}

local function species_name(id)
  return game.text(NAMES[id])
end

function professor(npc)
  npc:face_player()
  if game.flag("got_starter") then
    ui.say("text:lab.professor_after")
    return
  end
  ui.say("text:lab.professor_intro")
  local species
  repeat
    local pick = ui.choose({ "text:lab.pick_leafkit", "text:lab.pick_emberkit", "text:lab.pick_puddlefin" })
    if pick == nil then
      ui.say("text:lab.professor_later")
      return
    end
    if ui.yes_no("text:lab.confirm", { name = species_name(STARTERS[pick]) }) then
      species = STARTERS[pick]
    end
  until species

  game.give_creature(species, 5)
  game.set_flag("got_starter")
  game.set_var("starter", species)
  ui.say("text:lab.got_starter", { name = species_name(species) })

  local rival_species = RIVAL_PICK[species]
  game.set_var("rival_starter", rival_species)
  local rival = map.npc("rival")
  if rival then
    rival:face("up")
    ui.say("text:lab.rival_pick", { name = species_name(rival_species) })
    rival:move({ "down", "down", "down" })
    rival:hide()
  end
  ui.say("text:lab.professor_route")
end

function rival(npc)
  npc:face_player()
  ui.say("text:lab.rival_waiting")
end

function aide(npc)
  npc:face_player()
  ui.say("text:lab.aide")
end

-- Once the rival has left for the route, they are no longer in the lab.
events.on("map_enter", function(e)
  if e.map == "lab" and game.flag("got_starter") then
    local rival = map.npc("rival")
    if rival then
      rival:hide()
    end
  end
end)
