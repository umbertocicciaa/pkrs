-- Smoke scenario (design §14.4): the game boots on its start map, the player can walk, and
-- the overworld keeps running.
expect_state("Overworld")
expect_map("town")
expect_player_at(5, 7, "up")

walk("left", 2)
expect_player_at(3, 7, "left")
walk("right", 2)
expect_player_at(5, 7, "right")

wait_ticks(60)
expect_state("Overworld")
expect(not locked(), "nothing should lock the player")
