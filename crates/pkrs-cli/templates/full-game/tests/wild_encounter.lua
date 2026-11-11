-- Wild creatures: after picking a starter, pace in the route's tall grass until a wild
-- battle starts, then run away.

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
  choose(1) -- Leafkit
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
walk("down", 4)
wait_until(function() return map() == "town" end, 120, "the town")
wait_for_state("Overworld")
walk("down", 1)
walk("right", 5)
walk("up", 7)
expect_map("route")

-- Into the south-west patch of tall grass (x 1..6, y 9..11), then pace until something
-- jumps out.
walk("up", 4)
walk("left", 3)
local steps = 0
while state() ~= "Battle" do
  walk(steps % 2 == 0 and "left" or "right", 1)
  steps = steps + 1
  expect(steps <= 200, "a wild creature should appear within 200 steps")
end
advance_battle()
expect(battle().opponent.name ~= nil, "a wild opponent")
choose(4) -- RUN
while state() == "Battle" do
  advance_battle()
end
wait_for_state("Overworld")
expect_map("route")
