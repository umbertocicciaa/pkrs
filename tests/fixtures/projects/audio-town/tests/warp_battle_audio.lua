-- Acceptance scenario (design §11, §15.2): map BGM across warps and battle music. The same
-- BGM id keeps playing across maps; a different id crossfades over 30 ticks; a battle pauses
-- the town track, plays the battle intro then its loop, and after the victory jingle the
-- town track resumes where it stopped.
local log = {}
local function expect_audio(more, what)
  for _, line in ipairs(more) do
    log[#log + 1] = line
  end
  expect_eq(audio_log(), log, what)
end

expect_eq(audio_log(), {}, "commands reach the backend only when the tick flushes them")
wait_ticks(1)
expect_audio({ "play #1 bgm:town fade=0" }, "the start map's BGM plays on the first tick")

-- Town -> house: another id crossfades.
walk("right", 4)
walk("down", 1) -- onto the house door at (9, 8)
wait_for_state("Overworld")
expect_map("house")
expect_audio({ "stop #1 fade=30", "play #2 bgm:house fade=30" }, "house crossfade")

-- House -> town: crossfade back.
walk("down", 3) -- onto the house exit at (2, 5)
wait_for_state("Overworld")
expect_map("town")
expect_player_at(9, 7, "up")
expect_audio({ "stop #2 fade=30", "play #3 bgm:town fade=30" }, "town crossfade")

-- Town -> yard -> town: the same id keeps playing, nothing is sent.
walk("left", 8)
walk("down", 1) -- onto the yard gate at (1, 8)
wait_for_state("Overworld")
expect_map("yard")
walk("down", 3)
wait_for_state("Overworld")
expect_map("town")
expect_player_at(1, 7, "up")
expect_audio({}, "same BGM id keeps playing")

-- The rival: a script SFX, then the trainer battle.
walk("right", 4)
walk("up", 2)
expect_player_at(5, 5, "up")
press("confirm")
expect_dialogue("Let's battle!")
expect_audio({ "play #4 sfx:exclaim fade=0" }, "audio.play_sfx from the map script")
advance_dialogue()
wait_for_state("Battle")
expect_audio(
  { "pause #3 fade=0", "play #5 bgm:battle_trainer:intro fade=0" },
  "battle start pauses the town track and plays the intro"
)
wait_until(function() return #audio_log() > #log end, 120, "the battle loop after the intro")
expect_audio({ "play #6 bgm:battle_trainer fade=0" }, "the intro hands over to the loop")

advance_battle() -- intro texts
while state() == "Battle" do
  choose(1) -- FIGHT
  choose(1) -- Ember
  advance_battle()
end
wait_until(function() return #audio_log() > #log + 1 end, 10, "the victory jingle")
expect_audio({ "stop #6 fade=0", "play #7 jingle:victory fade=0" }, "battle end plays the victory jingle")
wait_until(function() return #audio_log() > #log end, 120, "the town track to resume")
expect_audio({ "resume #3 fade=15" }, "the town track resumes where it stopped")

wait_for_state("Dialogue")
expect_dialogue("You beat me!")
advance_dialogue()
wait_for_state("Overworld")
expect_flag("beat_rival")
