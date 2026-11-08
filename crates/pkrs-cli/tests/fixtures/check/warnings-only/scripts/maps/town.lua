-- `greet` never records that it ran (warning); `welcome` does (no warning).
function greet()
  ui.say("Hello!")
end

local function remember()
  game.set_flag("welcomed", true)
end

function welcome()
  ui.say("Welcome!")
  game.set_var("visits", 1)
end

events.on("tick", function()
  -- a per-tick listener: performance warning
end)
