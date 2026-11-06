-- Coroutine cutscene fixture (§9.3): waits in ticks and records what it emits.
emitted = {}

local function emit(name)
  emitted[#emitted + 1] = name .. "@" .. time.ticks()
end

function on_interact(npc)
  emit("start:" .. npc)
  wait(30)
  emit("after_wait")
  time.wait(10)
  emit("done")
end

function wander()
  for _ = 1, 3 do
    wait(20)
    emit("wander")
  end
end

function broken_cutscene()
  emit("broken_start")
  wait(5)
  error("cutscene failed")
end

function emitted_log()
  return table.concat(emitted, ",")
end
