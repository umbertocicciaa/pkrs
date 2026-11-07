-- Mudworks: a terrain, a field action and a move, all from a plugin (§9.4).
local P = plugin.define()

-- A move registered from Lua (the folder adds `mud_slap`).
data.register_move {
  id = "mudworks:mud_bomb", type = "ground", category = "special",
  power = 65, accuracy = 85, priority = 0,
  effects = { { effect = "damage" } },
}

-- Mud can only be crossed wearing waders (§6.3).
world.register_terrain("mud", function(ctx)
  return ctx.field["mudworks:waders"] == true
end)

-- Party menu: put the waders on.
world.register_field_action("mudworks:waders", {
  label = "text:mudworks.waders",
  run = function(ctx)
    player.set_field_state("mudworks:waders", true)
    plugin.save.waders_used = (plugin.save.waders_used or 0) + 1
  end,
})

-- Facing a mud tile and pressing Confirm: inspect it.
world.register_field_action("mudworks:probe", {
  terrain = "mud",
  menu = false,
  run = function(ctx)
    game.set_var("probed", ctx.target.x .. "," .. ctx.target.y)
  end,
})

-- A custom input action and its event.
input.register_action("mudworks:whistle", { "m" })
events.on("action", function(e)
  if e.action == "mudworks:whistle" then
    game.set_flag("whistled", true)
  end
end)

print("plugin init: " .. P.id .. " " .. P.version)
return P
