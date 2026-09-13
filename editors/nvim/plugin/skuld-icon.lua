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

local function register_mini()
  if registered.mini or _G.MiniIcons == nil then
    return
  end
  local ok, mini = pcall(require, "mini.icons")
  if not ok then
    return
  end
  registered.mini = true
  -- `mini.icons` keeps its table in the config it was set up with, and merging
  -- into `MiniIcons.config` is the supported way to add one. This runs after
  -- its own `setup()`, never before: the earlier call would be overwritten.
  mini.setup(vim.tbl_deep_extend("force", _G.MiniIcons.config or {}, {
    extension = { skuld = { glyph = GLYPH, hl = "MiniIconsPurple" } },
    filetype = { skuld = { glyph = GLYPH, hl = "MiniIconsPurple" } },
  }))
end

local function register_devicons()
  local devicons = package.loaded["nvim-web-devicons"]
  if registered.devicons or devicons == nil or type(devicons.set_icon) ~= "function" then
    return
  end
  registered.devicons = true
  devicons.set_icon({
    skuld = { icon = GLYPH, color = COLOR, cterm_color = CTERM, name = "Skuld" },
  })
  if type(devicons.set_icon_by_filetype) == "function" then
    devicons.set_icon_by_filetype({ skuld = "skuld" })
  end
end

local function register()
  register_mini()
  register_devicons()
  return registered.mini and registered.devicons
end

-- A provider is normally lazy: it loads the first time something asks it to
-- draw, which is after this file has run. Waiting for that moment is not
-- enough, because the request that loads it is also the request that wants the
-- icon — the file tree draws its first line before we could be told. So load
-- one on purpose, once Neovim has finished starting. Under a plugin manager
-- the `require` is what runs the provider's own `setup()`, and this code runs
-- immediately after it, still ahead of anything that draws.
local function load_provider()
  if _G.MiniIcons == nil and package.loaded["nvim-web-devicons"] == nil then
    if not pcall(require, "mini.icons") then
      pcall(require, "nvim-web-devicons")
    end
  end
  register()
end

register()

local group = vim.api.nvim_create_augroup("SkuldIcon", { clear = true })

-- `VeryLazy` is LazyVim's "startup is over"; `VimEnter` is the same moment for
-- everyone else. Whichever arrives first does the work, and the second finds
-- nothing left to do.
vim.api.nvim_create_autocmd("User", {
  group = group,
  pattern = "VeryLazy",
  callback = load_provider,
})
vim.api.nvim_create_autocmd("VimEnter", {
  group = group,
  callback = load_provider,
})

-- And if something loaded a provider before either of those, take it then:
-- lazy.nvim announces a plugin it has loaded *and* configured with `LazyLoad`.
vim.api.nvim_create_autocmd("User", {
  group = group,
  pattern = "LazyLoad",
  callback = function(event)
    if event.data == "mini.icons" or event.data == "nvim-web-devicons" then
      register()
    end
  end,
})
