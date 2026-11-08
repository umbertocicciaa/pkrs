-- Game-level save migration (§12): runs after the engine migrations when the save was
-- written by another game version.
function migrate(save, from)
  local flags = save.world.flags
  if flags.sailor_met ~= nil then
    flags.met_sailor, flags.sailor_met = flags.sailor_met, nil
  end
  save.world.vars.migrated_from = from
  return save
end
