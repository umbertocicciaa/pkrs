-- Plays the whole story with scripted input: the guide's choice, the walk to the cliff, the
-- keeper's question and the ending.
-- Run it with `pkrs-runtime --project . --script tests/story.lua`.

expect_state("Overworld")
expect_map("start")
expect_player_at(5, 7)

-- The guide stands at (11, 5): walk along the path and talk to them from below.
walk("right", 6)
walk("up", 1)
face("up")
press("confirm")
expect_dialogue("Oh! A new face in Willowmere.")
advance_dialogue()
wait_for_state("Choice")
expect_choices({ "Who are you?", "What should I do?", "Nothing, thanks." })
choose(2)
expect_dialogue("Climb the path to the north, past the gap in the trees.")
advance_dialogue()
expect_dialogue("The keeper is waiting on the northern cliff. Don't keep him waiting!")
advance_dialogue()
wait_for_state("Overworld")
expect_flag("met_guide")

-- North through the gap in the trees at (5, 0): the cliff map continues seamlessly.
walk("left", 4)
walk("up", 5)
walk("left", 2)
walk("up", 2)
expect_map("north")
expect_player_at(5, 5)

-- The keeper stands at (12, 3).
walk("up", 1)
walk("right", 7)
face("up")
press("confirm")
expect_dialogue("You came! The sun is about to set.")
advance_dialogue()
wait_for_state("Choice")
answer(true)
expect_dialogue("Then come. The best seat is by the edge.")
advance_dialogue()

-- The keeper walks to the edge, the screen fades and the ending plays.
wait_until(function() return map() == "ending" end, 600, "the ending map")
wait_for_state("Dialogue")
expect_dialogue("The sun sinks into the sea, and the first stars come out.")
advance_dialogue()
expect_dialogue("Keeper: Thank you for staying. Every story needs a good ending.")
advance_dialogue()
expect_dialogue("THE END")
advance_dialogue()
advance_dialogue()
wait_for_state("Overworld")
expect_flag("story_complete")
expect_player_at(7, 6)
