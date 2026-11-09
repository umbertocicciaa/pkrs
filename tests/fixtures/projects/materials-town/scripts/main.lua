-- Day/night cycle (design §5.6): Lua only sets material parameters; the `day_night` and
-- `water` materials read the `day_tint` global uniform.
local PHASES = {
  { 1.0, 1.0, 1.0, 1.0 },  -- day
  { 1.0, 0.8, 0.6, 1.0 },  -- dusk
  { 0.35, 0.4, 0.7, 1.0 }, -- night
}

-- Ticks each phase lasts.
PHASE_TICKS = 30

function day_night_cycle()
  local phase = 1
  while true do
    render.set_global_uniform("day_tint", PHASES[phase])
    time.wait(PHASE_TICKS)
    phase = phase % #PHASES + 1
  end
end

events.on("game_start", day_night_cycle)
