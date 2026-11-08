-- Save fixture plugin (§9.4, §12): keeps a counter, a list and a value with no saved form.
local P = plugin.define()
P.save.count = P.save.count or 0
P.save.history = P.save.history or { "start" }
-- Functions cannot be saved: the save system skips this key and logs it.
P.save.callback = function() return P.save.count end
return P
