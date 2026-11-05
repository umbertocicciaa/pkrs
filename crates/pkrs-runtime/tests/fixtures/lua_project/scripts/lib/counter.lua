local M = {}
local total = 0

function M.add(n)
  total = total + n
end

function M.total()
  return total
end

return M
