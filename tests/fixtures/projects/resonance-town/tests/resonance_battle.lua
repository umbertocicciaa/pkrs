-- Acceptance (§9.5, §15.2): Resonance adds a fight-menu entry, plays FX, and boosts
-- same-type damage once per battle.
expect_state("Overworld")
walk("up", 2)
press("confirm")
expect_dialogue("Focus your type!")
advance_dialogue()

wait_for_state("Battle")
advance_battle()
expect_eq(battle().menu, "action")
expect_eq(battle().turn, 1)

local labels = { "FIGHT", "BAG", "PARTY", "RUN", "RESONATE" }
expect_choices(labels)

-- Use Resonate once (custom action at the end of the action menu).
choose(5)
expect_dialogue("resonance.start")
advance_battle()

-- The entry is gone until the next battle.
expect_choices({ "FIGHT", "BAG", "PARTY", "RUN" })

-- Finish the wild battle with Ember (fire move, matches Emberkit's primary type while resonating).
local turns = 0
while state() == "Battle" do
  expect_eq(battle().menu, "action")
  choose(1)
  choose(1)
  turns = turns + 1
  expect(turns <= 8, "wild battle should end quickly")
  advance_battle()
end

wait_for_state("Dialogue")
expect_dialogue("Well done!")
advance_dialogue()
wait_for_state("Overworld")
expect_flag("tried_resonance")
expect_var("battle_outcome", "win")
