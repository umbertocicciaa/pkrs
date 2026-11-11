-- The whole game: pick Emberkit, leave the lab, walk up the route and beat the rival's
-- Leafkit with Ember. Run it with `pkrs-runtime --project . --script tests/rival_battle.lua`.

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
  choose(2) -- Emberkit
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
expect_flag("got_starter")

-- Out of the lab and north to the route.
walk("down", 4)
wait_until(function() return map() == "town" end, 120, "the town")
wait_for_state("Overworld")
expect_player_at(4, 5)
walk("down", 1)
walk("right", 5)
walk("up", 7)
expect_map("route")
expect_player_at(9, 15)

-- The rival waits at the end of the path; their trigger is at (9, 4).
walk("up", 11)
wait_for_state("Dialogue")
expect_dialogue("Rival: There you are! Let's see whose partner is stronger!")
advance_dialogue()
wait_for_state("Battle")
advance_battle()
expect_eq(battle().player.name, "Emberkit")
expect_eq(battle().opponent.name, "Leafkit")
local turns = 0
while state() == "Battle" do
  if battle().menu == "action" then
    choose(1) -- FIGHT
    choose(3) -- Ember: super effective on Leafkit
    turns = turns + 1
    expect(turns <= 10, "the battle should be over within ten turns")
  end
  advance_battle()
end
wait_for_state("Dialogue")
expect_dialogue("Rival: You won? I'll train harder. See you around!")
advance_dialogue()
wait_for_state("Overworld")
expect_flag("beat_rival")
