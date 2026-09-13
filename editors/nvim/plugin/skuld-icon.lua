-- The Skuld file icon, for whatever draws icons in this configuration.
--
-- The glyph is the Nerd Font hourglass (U+F254), the same mark as
-- `assets/skuld.svg`: Skuld is the norn of what is yet to come. It lives in
-- the Nerd Font private use area, so a terminal without a patched font shows
-- a box here and nothing else breaks.
--
-- Two icon providers exist and a configuration may have either: `mini.icons`
-- (what LazyVim installs, and what it makes `nvim-web-devicons` resolve to)
-- and `nvim-web-devicons` itself. Both are fed below; a missing one is not an
-- error. Registering the extension covers the file tree, the filetype covers
-- the statusline of a buffer whose name has no extension.

local GLYPH = "" -- U+F254
local COLOR = "#8A7CF0"
local CTERM = "141"

local registered = { mini = false, devicons = false }

-- Returns true once neither provider has anything left to be told.
local function register()
  -- `mini.icons` keeps its table in the config it was set up with, and merging
  -- into `MiniIcons.config` is the supported way to add one. Only do it once
  -- the plugin has actually been set up, or this would be overwritten by the
  -- setup call that comes later.
  if not registered.mini and _G.MiniIcons ~= nil then
    local ok, mini = pcall(require, "mini.icons")
    if ok then
      registered.mini = true
      mini.setup(vim.tbl_deep_extend("force", _G.MiniIcons.config or {}, {
        extension = { skuld = { glyph = GLYPH, hl = "MiniIconsPurple" } },
        filetype = { skuld = { glyph = GLYPH, hl = "MiniIconsPurple" } },
      }))
    end
  end

  -- `nvim-web-devicons` is only touched when it is already loaded: requiring it
  -- would pull the plugin in under a manager that had it lazy, and under
  -- LazyVim the require would land on the mock `mini.icons` installs anyway.
  local devicons = package.loaded["nvim-web-devicons"]
  if not registered.devicons and devicons ~= nil and type(devicons.set_icon) == "function" then
    registered.devicons = true
    devicons.set_icon({
      skuld = { icon = GLYPH, color = COLOR, cterm_color = CTERM, name = "Skuld" },
    })
    if type(devicons.set_icon_by_filetype) == "function" then
      devicons.set_icon_by_filetype({ skuld = "skuld" })
    end
  end

  return registered.mini and registered.devicons
end

register()

-- Startup order is not ours to control: the providers may load after this file,
-- and a lazy one loads when something first asks it to draw. Look again at both
-- of those moments, until there is nobody left to tell.
local group = vim.api.nvim_create_augroup("SkuldIcon", { clear = true })
vim.api.nvim_create_autocmd({ "VimEnter", "FileType" }, {
  group = group,
  pattern = "*",
  callback = register, -- returning true here deletes the autocommand
})
