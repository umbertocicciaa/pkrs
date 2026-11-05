-- Tries every way out of the sandbox; each attempt must fail.
local attempts = {
  function() return io.open("pkrs.toml") end,
  function() return os.execute("echo hi") end,
  function() return require("../../Cargo") end,
  function() return dofile("scripts/main.lua") end,
  function() return loadfile("scripts/main.lua") end,
  function() return package.loadlib("x", "y") end,
  function() return debug.getregistry() end,
  function() return getmetatable("").__index end,
}
local escaped = 0
for _, attempt in ipairs(attempts) do
  if pcall(attempt) then escaped = escaped + 1 end
end
return escaped
