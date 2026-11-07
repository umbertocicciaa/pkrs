-- Acceptance scenario (design §15.2): talk to NPC -> dialogue -> choice -> back to overworld.
expect_state("Overworld")
expect_npc_at("guide", 5, 4, "down")

walk("up", 2) -- stop right below the guide
expect_player_at(5, 5, "up")

press("confirm") -- talk
expect_state("Dialogue")
expect_dialogue("Welcome, traveller!")
expect(locked(), "talking locks the player")

advance_dialogue()
wait_for_state("Choice")
expect_choices({ "North", "South", "Nowhere" })
expect_eq(cursor(), 1)
choose(2) -- "South"

wait_for_state("Dialogue")
expect_dialogue("South it is.")
advance_dialogue()

wait_for_state("Overworld")
wait_until(function() return not locked() end, 10, "the player to be unlocked")
expect_var("destination", 2)
expect_flag("met_guide")
expect_player_at(5, 5, "up")

-- Talking again takes the other branch.
press("confirm")
expect_dialogue("Safe travels!")
advance_dialogue()
wait_for_state("Overworld")
