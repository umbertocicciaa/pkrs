-- Stepping on a warp fades to the house; the house's warp leads back.
walk("right", 4)
walk("down", 1) -- onto the door at (9, 8)
expect_state("Warp")
wait_for_state("Overworld")
expect_map("house")
expect_player_at(2, 2, "down")

walk("down", 3) -- onto the house exit at (2, 5)
wait_for_state("Overworld")
expect_map("town")
expect_player_at(9, 7, "up")
-- The town's map script counts map_enter: once at start, once on the way back.
expect_var("visits_town", 2)
