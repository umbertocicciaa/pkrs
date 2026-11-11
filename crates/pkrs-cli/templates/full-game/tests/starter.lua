-- Starter selection: walk into the lab, pick Emberkit from the professor and watch the rival
-- pick too. Run it with `pkrs-runtime --project . --script tests/starter.lua`.

wait_for_state("Dialogue")
expect_dialogue("Today is the day! The professor is waiting for you in the lab.")
advance_dialogue()
wait_for_state("Overworld")
expect_eq(#party(), 0, "the party starts empty")

-- From home to the lab door at (4, 4).
walk("down", 1)
walk("left", 12)
walk("up", 2)
wait_until(function() return map() == "lab" end, 120, "the lab")
wait_for_state("Overworld")
expect_player_at(4, 6)

-- The professor stands at (4, 2).
walk("up", 3)
face("up")
press("confirm")
expect_dialogue("Professor: There you are! Pick a partner. Choose carefully!")
advance_dialogue()
wait_for_state("Choice")
expect_choices({ "Leafkit (grass)", "Emberkit (fire)", "Puddlefin (water)" })
choose(2)
wait_for_state("Choice")
expect_dialogue("Professor: So you want Emberkit?")
answer(true)
expect_dialogue("Emberkit joined your party!")
advance_dialogue()
expect_dialogue("Rival: Then I'll take Leafkit! See you on the route!")
advance_dialogue()
-- The rival walks out of the lab before the professor speaks again.
wait_for_state("Dialogue")
expect_dialogue("Professor: Go north of town to the route and see how your partner fights!")
advance_dialogue()
wait_for_state("Overworld")
wait_until(function() return not locked() end, 60, "the player to be free")

expect_flag("got_starter")
expect_var("rival_starter", "leafkit")
expect_eq(#party(), 1)
expect_eq(party()[1].species, "emberkit")
expect_eq(party()[1].level, 5)
expect_eq(party()[1].moves, { "tackle", "glare", "ember" })
