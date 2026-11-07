-- Reading a sign opens its text without any script.
walk("left", 3)
walk("up", 2)
expect_player_at(2, 5, "up")

press("confirm")
expect_state("Dialogue")
expect_dialogue("Welcome to Scenario Town!")
advance_dialogue()
expect_state("Overworld")
expect_flag("met_guide", false)
