-- Acceptance scenario (design §6.3, §7.2, §15.2): walk in tall grass with the scenario's
-- fixed seed until a wild battle starts, run away, and be back in the overworld on the same
-- tile, free to walk on.
expect_state("Overworld")

-- Row 5 lies inside the encounter rect but is plain ground: the table only rolls on tall
-- grass, so pacing there never starts a battle.
walk("up", 2)
expect_player_at(5, 5, "up")
for _ = 1, 10 do
  walk("left")
  walk("right")
  expect_state("Overworld")
end
expect_player_at(5, 5, "right")

-- Into the grass, then pace left and right until something jumps out.
walk("up")
local steps = 1
local dir = "left"
while state() ~= "Battle" do
  expect(steps < 100, "a rate-20 rect should start a battle well within 100 steps")
  walk(dir)
  steps = steps + 1
  local x = player()
  if x <= 3 then dir = "right" elseif x >= 8 then dir = "left" end
end
local x, y = player()
expect(y == 4 and x >= 2 and x <= 9, "the battle started on a grass tile")

wait_until(function() return dialogue() ~= nil end, 10, "the first battle message")
local foe = battle().opponent
expect(foe.name == "Sproutling" or foe.name == "Leafkit", "a creature of the route's table")
if foe.name == "Sproutling" then
  expect(foe.level >= 2 and foe.level <= 4, "sproutling level in 2..4")
else
  expect(foe.level >= 3 and foe.level <= 5, "leafkit level in 3..5")
end
expect_dialogue("A wild " .. foe.name .. " appeared!")
expect_eq(battle().player.name, "Emberkit")

advance_battle() -- intro texts
expect_eq(battle().menu, "action")
expect_choices({ "FIGHT", "BAG", "PARTY", "RUN" })
choose(4) -- RUN
wait_until(function() return dialogue() ~= nil end, 30, "the run message")
expect_dialogue("Got away safely!")
advance_battle()

-- Back in the overworld where the battle started.
wait_for_state("Overworld")
expect_player_at(x, y)
expect(not locked(), "the player is free to move")
walk("down")
walk("down")
expect_player_at(x, 6)
expect_state("Overworld")
