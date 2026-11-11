-- The field menu (party, bag, save) opens and closes; Mom's Potions land in the bag.

local function get_starter()
  wait_for_state("Dialogue")
  advance_dialogue()
  wait_for_state("Overworld")
  walk("down", 1)
  walk("left", 12)
  walk("up", 2)
  wait_until(function() return map() == "lab" end, 120, "the lab")
  wait_for_state("Overworld")
  walk("up", 3)
  face("up")
  press("confirm")
  advance_dialogue()
  wait_for_state("Choice")
  choose(3) -- Puddlefin
  wait_for_state("Choice")
  answer(true)
  -- Read the professor's and the rival's lines until the cutscene lets go of the player.
  while state() ~= "Overworld" or locked() do
    if state() == "Dialogue" then
      advance_dialogue()
    else
      wait_ticks(1)
    end
  end
end

get_starter()
press("menu")
wait_for_state("Menu")
press("cancel")
wait_for_state("Overworld")

-- Mom stands at (15, 7), next to home: talk to her from the path above.
walk("down", 4)
wait_until(function() return map() == "town" end, 120, "the town")
wait_for_state("Overworld")
walk("down", 1)
walk("right", 11)
face("down")
press("confirm")
expect_dialogue("Mom: Your very own partner! Take these Potions, just in case.")
advance_dialogue()
wait_for_state("Overworld")
expect_item("potion", 2)
expect_flag("mom_potions")
