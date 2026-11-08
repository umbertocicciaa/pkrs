-- Doubles acceptance (issue #50): intro, per-slot action menus and one full turn.
expect_state("Overworld")
walk("up", 2)
expect_player_at(5, 5, "up")

press("confirm")
expect_dialogue("Let's battle!")
advance_dialogue()

wait_for_state("Battle")
expect_dialogue("Twins wants to battle!")
advance_battle(2000)

local function pick_move_for_active_slot()
  expect_eq(battle().menu, "action")
  choose(1)
  expect_eq(battle().menu, "fight")
  choose(2)
  while battle().menu == "target" do
    choose(1)
  end
end

pick_move_for_active_slot()
pick_move_for_active_slot()

local start = tick()
while state() == "Battle" and battle().menu == nil do
  expect(tick() - start < 6000, "turn playback should finish")
  if dialogue() ~= nil then
    press("confirm")
  else
    wait_ticks(1)
  end
end

expect(state() == "Battle", "doubles battle still in progress after one turn")
expect(battle().menu == "action" or battle().menu == "party", "menu for next turn or a switch")
