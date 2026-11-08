-- Acceptance scenario (design §7.2, §15.2): a script starts a trainer battle, the player
-- chooses moves with the keyboard, wins, and play returns to the overworld with the result
-- handed back to the script.
expect_state("Overworld")
walk("up", 2) -- stand right below the rival
expect_player_at(5, 5, "up")

press("confirm")
expect_dialogue("Let's battle!")
advance_dialogue()

wait_for_state("Battle")
wait_until(function() return dialogue() ~= nil end, 10, "the first battle message")
expect_dialogue("Rival wants to battle!")
expect_eq(battle().opponent.name, "Leafkit")
expect_eq(battle().player.name, "Emberkit")

advance_battle() -- intro texts
expect_eq(battle().menu, "action")
expect_eq(battle().turn, 1)
expect_choices({ "FIGHT", "BAG", "PARTY", "RUN" })

-- Fight menu shows the moves with PP; cancel goes back.
choose(1)
expect_eq(battle().menu, "fight")
expect_choices({ "Ember 20/20", "Tackle 20/20" })
press("cancel")
expect_eq(battle().menu, "action")

-- Ember until the rival runs out of creatures.
local turns = 0
local opponents = {}
while state() == "Battle" do
  expect_eq(battle().menu, "action")
  opponents[battle().opponent.name] = true
  choose(1) -- FIGHT
  choose(1) -- Ember
  turns = turns + 1
  expect(turns <= 6, "the battle should be over within six turns")
  advance_battle()
end
expect(opponents["Leafkit"] and opponents["Sproutling"], "the rival sent out its second creature")

-- Back in the overworld, the script got the result.
wait_for_state("Dialogue")
expect_dialogue("You beat me!")
advance_dialogue()
wait_for_state("Overworld")
wait_until(function() return not locked() end, 10, "the player to be unlocked")
expect_flag("beat_rival")
expect_var("battle_outcome", "win")
expect_var("battle_turns", turns)
expect_player_at(5, 5, "up")

-- Talking again does not start another battle.
press("confirm")
expect_dialogue("You're too strong!")
advance_dialogue()
wait_for_state("Overworld")
