-- Without a partner the north exit sends you back: the `route_gate` trigger in
-- scripts/maps/town.lua.

wait_for_state("Dialogue")
advance_dialogue()
wait_for_state("Overworld")

walk("down", 1)
walk("left", 7)
walk("up", 4)
expect_player_at(9, 2)
walk("up", 1)
wait_for_state("Dialogue")
expect_dialogue("A voice calls: \"Wait! Wild creatures live in the tall grass. Get a partner from the lab first!\"")
advance_dialogue()
wait_until(function() return state() == "Overworld" and not locked() end, 120, "the gate to let go")
expect_map("town")
expect_player_at(9, 2)
