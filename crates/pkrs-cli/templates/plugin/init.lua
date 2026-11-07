-- {{name}} (plugin `{{id}}`), docs/design.md §9.4.
--
-- This file runs once at start-up, in plugin load order, inside this plugin's own sandboxed
-- `_ENV` (§9.6). `require("lib.x")` loads `plugins/{{id}}/lib/x.lua`.

local P = plugin.define()   -- { id, name, version, api, description, save = {} }

-- Battle hooks (§7.4). Handlers run in ascending `priority` (default 0), then in plugin load
-- order. Return a modified value, `nil` to leave it unchanged, or, for cancellable hooks,
-- `{ cancel = true, message = "..." }`. `pkrs check --plugins` prints the resolved order.
battle.on("on_damage_calc", function(ctx, user, move, target, dmg)
  -- Example: make every hit 10% stronger.
  -- dmg.modifiers:push("{{id}}", 1.1)
  return dmg
end, { priority = 0 })

battle.on("on_end_of_turn", function(ctx)
  -- Runs at the end of every battle turn.
end)

-- World events (§9.2) follow the same priority rules.
events.on("map_enter", function(e)
  log.debug("{{id}}: entered " .. tostring(e.map))
end, { priority = 0 })

log.info("plugin " .. P.id .. " " .. P.version .. " loaded")

return P
