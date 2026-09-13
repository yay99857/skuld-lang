-- Server definition for `skuld-lsp`, discovered by Neovim 0.11+ from any
-- `lsp/<name>.lua` on the runtimepath. Activate it with `vim.lsp.enable("skuld")`;
-- defining it here does not start anything on its own.

-- Prefer an installed binary, and fall back to this checkout's release build so
-- the server works straight after `cargo build --release` with no install step.
local function command()
  if vim.fn.executable("skuld-lsp") == 1 then
    return { "skuld-lsp" }
  end
  -- `.../editors/nvim/lsp/skuld.lua` -> the repository root is four levels up.
  local root = vim.fn.fnamemodify(debug.getinfo(1, "S").source:sub(2), ":h:h:h:h")
  local built = root .. "/target/release/skuld-lsp"
  if vim.fn.executable(built) == 1 then
    return { built }
  end
  return { "skuld-lsp" }
end

return {
  cmd = command(),
  filetypes = { "skuld" },
  -- A Skuld program has no manifest: the root is the directory the entry file
  -- lives in, which is also how the compiler resolves every import path. Using
  -- a marker file would invent a project layout the language does not have.
  root_dir = function(bufnr, done)
    done(vim.fs.dirname(vim.api.nvim_buf_get_name(bufnr)))
  end,
  -- Inlay hints are off until something turns them on, and a server that has
  -- them and never shows them reads as a server that does not have them. Turn
  -- them off again for a buffer with
  -- `vim.lsp.inlay_hint.enable(false, { bufnr = 0 })`.
  on_attach = function(client, bufnr)
    if client:supports_method("textDocument/inlayHint") then
      vim.lsp.inlay_hint.enable(true, { bufnr = bufnr })
    end
  end,
}
