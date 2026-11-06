-- Lua AI fixture (§7.5): a champion that plays type matchups, heals once when low and
-- switches out of a revealed super-effective threat. Ties are broken with math.random, which
-- draws from the AI's own deterministic stream.

local chart = {
  fire = { grass = 2, water = 0.5, fire = 0.5 },
  water = { fire = 2, grass = 0.5, water = 0.5 },
  grass = { water = 2, fire = 0.5, grass = 0.5 },
  normal = { ghost = 0 },
}

local function effectiveness(move_type, target_types)
  local m = 1
  for _, t in ipairs(target_types) do
    local row = chart[move_type]
    if row and row[t] then m = m * row[t] end
  end
  return m
end

local function best_move(me, foe)
  local best, best_score = {}, -1
  for _, mv in ipairs(me.moves) do
    if mv.pp > 0 then
      local score = (mv.power or 0) * effectiveness(mv.type or "normal", foe.types)
      for _, t in ipairs(me.types) do
        if t == mv.type then score = score * 1.5 end
      end
      if score > best_score then
        best, best_score = { mv }, score
      elseif score == best_score then
        best[#best + 1] = mv
      end
    end
  end
  if #best == 0 then return nil, 0 end
  return best[math.random(1, #best)], best_score
end

-- Worst multiplier the foe's *revealed* moves have against `c`.
local function threat(foe, c)
  local worst = 0
  for _, mv in ipairs(foe.moves) do
    if mv.type and mv.category ~= "status" then
      local e = effectiveness(mv.type, c.types)
      if e > worst then worst = e end
    end
  end
  return worst
end

local function bench(view)
  local out = {}
  for _, c in ipairs(view.own.party) do
    if not c.active and not c.fainted then out[#out + 1] = c end
  end
  return out
end

local function first_foe(view)
  for i = 1, view.side_size do
    local f = view.foe.active[i]
    if f and not f.fainted then return f end
  end
end

function choose_action(view)
  local foe = first_foe(view)

  if view.request == "replace" then
    local pick, pick_score = nil, -1
    for _, c in ipairs(bench(view)) do
      local _, score = best_move(c, foe or { types = {} })
      if score > pick_score then pick, pick_score = c, score end
    end
    if pick then return { switch = pick } end
    return nil
  end

  local me = view.me
  if not foe then return nil end

  if me.hp_percent <= 30 and view.items[1] then
    return { item = view.items[1], target = me }
  end

  if threat(foe, me) >= 2 then
    for _, c in ipairs(bench(view)) do
      if threat(foe, c) <= 0.5 then return { switch = c.party_index } end
    end
  end

  local mv = best_move(me, foe)
  if not mv then return nil end -- let the built-in tier decide
  return { move = mv.id, target = foe }
end
