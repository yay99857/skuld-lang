-- The Skuld file icon, for whatever draws icons in this configuration.
--
-- The glyph is a boxed S (U+F0B1A), the same mark as `assets/skuld.svg`: a
-- letter cut in straight strokes, which is the one thing a rune and a file
-- icon at sixteen pixels agree on. It lives in the Nerd Font private use
-- area, so a terminal without a patched font shows a box here and nothing
-- else breaks.
--
-- Two icon providers exist and a configuration may have either: `mini.icons`
-- (what LazyVim installs, and what it makes `nvim-web-devicons` resolve to)
-- and `nvim-web-devicons` itself. Both are fed below; a missing one is not an
-- error. Registering the extension covers the file tree, the filetype covers
-- the statusline of a buffer whose name has no extension.

-- Built from its codepoint rather than written literally: this is a private
-- use area character, and a pipeline that does not know that — a patch, a
-- copy through a terminal — silently drops it, leaving an empty glyph that
-- draws as nothing at all.
local GLYPH = vim.fn.nr2char(0xF0B1A) -- nf-md-alpha_s_box
local COLOR = "#8A7CF0"
local CTERM = "141"

-- `mini.icons` keeps its table in the config it was set up with, and merging
-- into `MiniIcons.config` is the supported way to add one. Read the entry back
-- rather than remembering that we wrote it: a later `setup()` elsewhere drops
-- it, and then it has to be written again.
local function register_mini()
  if _G.MiniIcons == nil then
    return false
  end
  local config = _G.MiniIcons.config or {}
  local entry = config.extension and config.extension.skuld
  if entry ~= nil and entry.glyph == GLYPH then
    return true
  end
  local ok, mini = pcall(require, "mini.icons")
  if not ok then
    return false
  end
  mini.setup(vim.tbl_deep_extend("force", config, {
    extension = { skuld = { glyph = GLYPH, hl = "MiniIconsPurple" } },
    filetype = { skuld = { glyph = GLYPH, hl = "MiniIconsPurple" } },
  }))
  return true
end

local function register_devicons()
  local devicons = package.loaded["nvim-web-devicons"]
  if devicons == nil or type(devicons.set_icon) ~= "function" then
    return false
  end
  devicons.set_icon({
    skuld = { icon = GLYPH, color = COLOR, cterm_color = CTERM, name = "Skuld" },
  })
  if type(devicons.set_icon_by_filetype) == "function" then
    devicons.set_icon_by_filetype({ skuld = "skuld" })
  end
  return true
end

-- A provider is normally lazy: it loads the first time something asks it to
-- draw. Waiting for that moment is too late — the request that loads it is the
-- file tree asking for this very icon, and what it draws is what it caches. So
-- load one here, while Neovim is still starting and nothing has drawn yet.
-- Under a plugin manager the `require` is what runs the provider's own
-- `setup()`, and this registration follows it in the same tick.
local function register()
  if _G.MiniIcons == nil and package.loaded["nvim-web-devicons"] == nil then
    if not pcall(require, "mini.icons") then
      pcall(require, "nvim-web-devicons")
    end
  end
  local mini = register_mini()
  local devicons = register_devicons()
  return mini and devicons
end

register()

-- Then again at each later point a provider can have appeared, or have been
-- set up a second time by somebody else, which drops what was registered here.
-- `VeryLazy` is LazyVim's "startup is over", `LazyLoad` is lazy.nvim naming a
-- plugin it has just loaded and configured, and `VimEnter` covers neither.
local group = vim.api.nvim_create_augroup("SkuldIcon", { clear = true })
vim.api.nvim_create_autocmd("User", {
  group = group,
  pattern = { "VeryLazy", "LazyLoad" },
  callback = register,
})
vim.api.nvim_create_autocmd({ "VimEnter", "FileType" }, {
  group = group,
  pattern = "*",
  callback = register,
})
