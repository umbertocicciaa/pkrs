-- Binds this plugin to its manifest and records it in this plugin's own _ENV.
local P = plugin.define()
loaded_as = P.id
P.aura = require("lib.aura")
print("plugin init: " .. P.id .. " " .. P.version)
return P
