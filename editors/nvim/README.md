# Skuld for Neovim

Syntax highlighting for `.skuld` files, and the client side of `skuld-lsp`.
This is the parallel tooling track `ROADMAP.md` describes; it is not part of
any milestone and blocks none.

```
syntax/skuld.vim      highlighting
ftdetect/skuld.vim    *.skuld -> filetype skuld
lsp/skuld.lua         how to launch skuld-lsp (Neovim 0.11+)
```

## What it covers

Keywords, types, the prelude bindings, builtin methods, comments, numbers,
strings with `${ }` interpolation, and char literals.

Everything here is taken from the compiler rather than from the roadmap:
the keyword list mirrors `compiler/src/lexer.rs`, the prelude mirrors
`compiler/src/resolver.rs`, and the escape set is the exact one the lexer
accepts — a `\q` is shown as an error because `E0004` is what the compiler
would report.

`interface` and `impl` are highlighted as keywords because the lexer produces
tokens for them. Their milestone is unimplemented, and highlighting them is
not a claim otherwise.

## What it does not cover

No semantic knowledge: a capitalised name reads as a type by convention, not
because anything resolved it, and an undefined name looks like any other. That
is the language server's job, not a regex highlighter's.

One approximation: the lexer counts brace depth inside `${ }` properly, while
this file tracks one level of nesting. A record literal inside an interpolation
highlights correctly; a record literal inside a record literal inside an
interpolation does not.

## The language server

`lsp/skuld.lua` defines how to start `skuld-lsp`; Neovim 0.11 and later find it
on the runtimepath. Defining a server does not start it, so a plugin spec has
to enable it:

```lua
vim.lsp.enable("skuld")
```

It looks for `skuld-lsp` on `PATH` first and falls back to this checkout's
`target/release/skuld-lsp`, so `cargo build --release` is enough with no
install step. Build it before expecting diagnostics.

A Skuld program has no manifest file, so the root is the directory the open
file lives in — which is also how the compiler resolves every import path.

## The language server

`skuld-lsp` reports the compiler's diagnostics as you type, and completes names
after a `.` or at a bare cursor. Build it first — without it every `.skuld`
buffer opens with a client error, since the config below points at a binary:

```sh
cargo install --path ~/www/skuld-lang/lsp   # the stable path
cargo build --release -p skuld-lsp          # the fallback this config also accepts
```

`lsp/skuld.lua` prefers `skuld-lsp` on `PATH` and falls back to the checkout's
`target/release` build, which `cargo clean` removes — which is why installing is
the documented path. Neovim 0.11+ discovers that file on any runtimepath entry;
`vim.lsp.enable("skuld")` is what starts it.

What it answers: diagnostics on open and on every keystroke, including for the
files an open file imports, and completion — variants after an enum name,
exports after an import qualifier, fields and methods after a value, and
keywords, the prelude and what is in scope otherwise. What it does not answer
yet: hover, go-to-definition and find-references, so `K` and `gd` stay silent.

Two behaviours worth knowing before filing a bug. Completion answers from the
last check that **succeeded**, because a half-typed line rarely parses; a file
that has never checked offers keywords only. And the compiler stops at the
first stage that fails, so a file with a resolver error and a type error shows
only the first until it is fixed.

`:checkhealth vim.lsp` lists the client, its command and the buffers it is
attached to; `<leader>cl` is LazyVim's own view of the same thing.

## Install

With `lazy.nvim`, pointing at a local checkout. Do **not** lazy-load on
`ft = "skuld"`: this plugin is what teaches Neovim that filetype, so waiting
for it is circular and nothing ever loads.

```lua
{
  dir = "~/www/skuld-lang/editors/nvim",
  name = "skuld.nvim",
  lazy = false,
  init = function()
    vim.filetype.add({ extension = { skuld = "skuld" } })
  end,
  config = function()
    vim.lsp.enable("skuld")
  end,
}
```

Or without a plugin manager:

```sh
mkdir -p ~/.config/nvim/syntax ~/.config/nvim/ftdetect ~/.config/nvim/lsp
ln -s ~/www/skuld-lang/editors/nvim/syntax/skuld.vim ~/.config/nvim/syntax/
ln -s ~/www/skuld-lang/editors/nvim/ftdetect/skuld.vim ~/.config/nvim/ftdetect/
ln -s ~/www/skuld-lang/editors/nvim/lsp/skuld.lua ~/.config/nvim/lsp/
```

## Maintaining it

A milestone that adds a keyword has to add it here too. The check that catches
a drift is comparing the `syn keyword` lines against the match arm in
`Lexer::identifier` (`compiler/src/lexer.rs`); there is no automated test
tying the two together yet.

Vim resolves competing syntax items by **definition order — the last one
wins**, which is why the general pattern is always declared before the
specific one it should lose to (`skuldNumber` before `skuldFloat`,
`skuldFunction` before `skuldMethod`, `skuldEscapeError` before
`skuldEscape`). Reordering those blocks silently breaks them.

Write patterns with `/.../` delimiters, never as `"..."` strings: vimscript
processes escapes inside a double-quoted string first, which turns `\|` into a
literal `|` and quietly destroys any alternation.
