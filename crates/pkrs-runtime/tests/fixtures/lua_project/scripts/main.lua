-- Fixture game script: required module, engine RNG and per-tick callback.
local counter = require("lib.counter")

ticks = 0

function tick()
  ticks = ticks + 1
  counter.add(math.random(1, 6))
end

function summary()
  return {
    ticks = ticks,
    total = counter.total(),
    sandboxed = io == nil and os == nil and package == nil and debug == nil,
  }
end
