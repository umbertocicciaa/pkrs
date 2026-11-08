-- Localization (design §5.5): the same dialogue in Italian (the default) and English, with a
-- runtime switch through the options API (`game.set_language`).
expect_state("Overworld")
expect_eq(language(), "it", "the default language is active")

-- A sign's `text:` key, in Italian.
face("right")
press("confirm")
expect_dialogue("Cartello: solo in italiano.")
advance_dialogue()
wait_for_state("Overworld")

-- The guide speaks Italian; arguments fill the placeholders; engine texts are Italian too.
face("up")
walk("up", 2)
press("confirm")
expect_dialogue("Benvenuto a Borgo Lingua!")
advance_dialogue()
wait_for_state("Choice")
expect_dialogue("Vuoi passare a inglese?")
expect_choices({ "Sì", "No" })
answer(true)

-- The switch applies at once: the next text box is English.
wait_for_state("Dialogue")
expect_eq(language(), "en")
expect_dialogue("Language switched 1 times. Bye!")
advance_dialogue()
wait_for_state("Overworld")
wait_until(function() return not locked() end, 10, "the player to be unlocked")

-- The same dialogue again, now in English.
press("confirm")
expect_dialogue("Welcome to Borgo Lingua!")
advance_dialogue()
wait_for_state("Choice")
expect_dialogue("Switch to Italian?")
expect_choices({ "Yes", "No" })
answer(false)
wait_for_state("Dialogue")
expect_dialogue("Language switched 1 times. Bye!")
advance_dialogue()
wait_for_state("Overworld")
wait_until(function() return not locked() end, 10, "the player to be unlocked")

-- A key English lacks falls back to the default language.
walk("down", 2)
face("right")
press("confirm")
expect_dialogue("Cartello: solo in italiano.")
advance_dialogue()
wait_for_state("Overworld")
expect_eq(language(), "en")
