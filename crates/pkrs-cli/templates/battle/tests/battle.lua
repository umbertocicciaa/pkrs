-- The game opens in a trainer battle; win it, decline the rematch, then talk to the rival
-- for a second battle.

local function win_battle()
  wait_for_state("Battle")
  advance_battle()
  expect_eq(battle().player.name, "Emberkit")
  local turns = 0
  while state() == "Battle" do
    if battle().menu == "action" then
      choose(1) -- FIGHT
      -- Ember is super effective on Leafkit but resisted by water: Tackle Puddlefin.
      choose(battle().opponent.name == "Puddlefin" and 2 or 1)
      turns = turns + 1
      expect(turns <= 10, "the battle should be over within ten turns")
    end
    advance_battle()
  end
  wait_for_state("Dialogue")
  expect_dialogue("You won in " .. turns .. " turns!")
  advance_dialogue()
end

win_battle()

wait_for_state("Choice")
expect_choices({ "Yes", "No" })
answer(false)
expect_dialogue("Talk to the rival when you want another battle.")
advance_dialogue()
wait_for_state("Overworld")

walk("up", 2) -- stand right below the rival
press("confirm")
expect_dialogue("Let's battle!")
advance_dialogue()
win_battle()
