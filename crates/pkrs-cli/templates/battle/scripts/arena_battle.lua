-- Fights the rival (`data/trainers/rival.ron`) until the player stops asking for a rematch.
-- Every battle starts with full HP and PP. Must run inside a script task.
return function()
  repeat
    local result = battle.start_trainer("rival")
    if result == nil then
      return -- the battle could not be built; the reason is in the log
    end
    if result.outcome == "win" then
      ui.say("You won in " .. result.turns .. " turns!")
    else
      ui.say("You lost. Better luck next time!")
    end
  until not ui.yes_no("Battle again?")
  ui.say("Talk to the rival when you want another battle.")
end
