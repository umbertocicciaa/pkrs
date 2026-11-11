-- Compatibility shims: reported by pkrs upgrade, not rewritten.
audio.jingle("fanfare")
map.register_terrain("ice", function(ctx)
  return "allow"
end)
