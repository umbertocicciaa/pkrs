-- Acceptance scenario (design §7.3, §7.4, §8.1, §15.2): a won trainer battle gives exp
-- (doubled by the game's `on_battle_end` hook), the player's Sproutling grows to level 6,
-- replaces a move with Vine Whip through the keyboard prompt, evolves into Bloomling in the
-- evolution scene, and play returns to the overworld. A second battle starts with the
-- evolved creature and its new moves and exp.

-- Confirms battle messages until the next battle menu opens or the first reward text
-- ("... gained N Exp. Points!") is shown. (`advance_battle` would read through the reward
-- texts too, up to the move prompt.)
local function play_turn()
  local start = tick()
  while battle().menu == nil and not (dialogue() or ""):find("Exp. Points!", 1, true) do
    expect(tick() - start < 600, "the turn should play back within 600 ticks")
    if dialogue() ~= nil then press("confirm") else wait_ticks(1) end
  end
end

-- Confirms messages until `text` is shown; fails on a menu or after 600 ticks.
local function read_until(text)
  local start = tick()
  while dialogue() ~= text do
    if tick() - start >= 600 or choices() ~= nil then
      fail("expected the text `" .. text .. "`, got `" .. tostring(dialogue()) .. "`")
    end
    if dialogue() ~= nil then press("confirm") else wait_ticks(1) end
  end
end

-- Confirms messages until `pred()` holds (fails after 600 ticks).
local function confirm_until(pred, what)
  local start = tick()
  while not pred() do
    expect(tick() - start < 600, "timed out waiting for " .. what)
    if dialogue() ~= nil and choices() == nil then press("confirm") else wait_ticks(1) end
  end
end

expect_state("Overworld")
expect_eq(#party(), 0, "the party loads with the first battle")

-- First battle: Sproutling (level 5, 125 exp) against the coach's level 5 Pebblit.
walk("up", 2)
press("confirm")
expect_dialogue("Show me your training!")
advance_dialogue()
wait_for_state("Battle")
advance_battle()
expect_eq(battle().player.name, "Sproutling")
expect_eq(battle().player.level, 5)
expect_eq(battle().opponent.name, "Pebblit")
-- No explicit moves in player.ron: the learnset's last four moves up to level 5.
choose(1)
expect_choices({ "Tackle 20/20", "Scratch 20/20", "Pound 20/20", "Leafage 20/20" })
press("cancel")

local m = party()[1]
expect(m.species == "sproutling" and m.level == 5 and m.exp == 125)
expect_eq(m.moves, { "tackle", "scratch", "pound", "leafage" })
local hp_before = m.max_hp

local turns = 0
while battle().menu == "action" do
  choose(1) -- FIGHT
  choose(4) -- Leafage
  turns = turns + 1
  expect(turns <= 6, "the battle should be over within six turns")
  play_turn()
end

-- Rewards play over the battle scene: 50 base exp · level 5 / 7 · 1.5 (trainer) = 52,
-- doubled by on_battle_end = 104.
expect_state("Battle")
read_until("Sproutling gained 104 Exp. Points!")
read_until("Sproutling grew to level 6!")
expect_eq(party()[1].level, 6)
expect_eq(party()[1].exp, 229)
expect(party()[1].max_hp > hp_before, "stats are recalculated on level up")
read_until("Sproutling wants to learn Vine whip, but Sproutling already knows 4 moves.")

-- The replace prompt: forget Tackle for Vine Whip, with the keyboard.
confirm_until(function() return choices() ~= nil end, "the replace prompt")
expect_state("Battle")
expect_dialogue("Which move should Sproutling forget?")
expect_choices({ "Tackle", "Scratch", "Pound", "Leafage", "Don't learn Vine whip" })
choose(1)
read_until("1, 2 and... Poof!")
read_until("Sproutling forgot Tackle and learned Vine whip!")
expect_eq(party()[1].moves, { "vine_whip", "scratch", "pound", "leafage" })

-- The evolution scene.
confirm_until(function() return state() == "Evolution" end, "the evolution scene")
expect_dialogue("What? Sproutling is evolving!")
expect_eq(party()[1].species, "sproutling")
confirm_until(function() return dialogue() == nil end, "the evolution animation")
expect_state("Evolution")
wait_until(function() return dialogue() ~= nil end, 300, "the end of the evolution animation")
expect_state("Evolution")
expect_dialogue("Congratulations! Your Sproutling evolved into Bloomling!")
m = party()[1]
expect(m.species == "bloomling" and m.name == "Bloomling" and m.level == 6)
expect_eq(m.moves, { "vine_whip", "scratch", "pound", "leafage" })

-- Back in the overworld; the script got the result with the modded exp.
confirm_until(function() return state() == "Dialogue" end, "the coach's reply")
expect_dialogue("Well fought!")
advance_dialogue()
wait_for_state("Overworld")
wait_until(function() return not locked() end, 10, "the player to be unlocked")
expect_var("battle_outcome", "win")
expect_var("battle_exp", 104)
expect_var("wins", 1)

-- Second battle: the session keeps the evolved creature, its level, moves and exp.
expect_player_at(5, 5, "up")
press("confirm")
expect_dialogue("Show me your training!")
advance_dialogue()
wait_for_state("Battle")
advance_battle()
expect_eq(battle().player.name, "Bloomling")
expect_eq(battle().player.level, 6)
expect_eq(battle().player.max_hp, party()[1].max_hp)
choose(1)
expect_choices({ "Vine whip 20/20", "Scratch 20/20", "Pound 20/20", "Leafage 20/20" })
press("cancel")
turns = 0
while battle().menu == "action" do
  choose(1)
  choose(1) -- Vine Whip
  turns = turns + 1
  expect(turns <= 6, "the battle should be over within six turns")
  play_turn()
end
read_until("Bloomling gained 104 Exp. Points!")
-- 229 + 104 = 333 < 343: no level up, no evolution this time.
confirm_until(function() return state() ~= "Battle" end, "the end of the battle")
expect_state("Dialogue")
expect_dialogue("Well fought!")
expect_eq(party()[1].level, 6)
expect_eq(party()[1].exp, 333)
advance_dialogue()
wait_for_state("Overworld")
expect_var("wins", 2)
