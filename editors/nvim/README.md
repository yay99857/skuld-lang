# Skuld for Neovim

Syntax highlighting for `.skuld` files, and the client side of `skuld-lsp`.
This is the parallel tooling track `ROADMAP.md` describes; it is not part of
any milestone and blocks none.

```
syntax/skuld.vim      highlighting
ftdetect/skuld.vim    *.skuld -> filetype skuld
lsp/skuld.lua         how to launch skuld-lsp (Neovim 0.11+)
plugin/skuld-icon.lua the file icon, for the tree and the statusline
```

## What it covers

Keywords, types, the prelude bindings, builtin methods, comments, numbers,
strings with `${ }` interpolation, and char literals.

Everything here is taken from the compiler rather than from the roadmap:
the keyword list mirrors `compiler/src/lexer.rs`, the prelude mirrors
`compiler/src/resolver.rs`, and the escape set is the exact one the lexer
accepts — a `\q` is shown as an error because `E0004` is what the compiler
would report.

`impl` is highlighted as a keyword because the lexer produces a token for it.
It has no meaning in the language yet, and highlighting it is not a claim
otherwise. `union`, `packed` and `align` are keywords in one position each and
ordinary identifiers everywhere else, which a regex highlighter cannot tell
apart reliably, so they are left uncoloured rather than coloured wrongly.

That the lists here match the compiler is checked rather than intended:
`cli/tests/editors.rs` reads the lexer's keyword table and the resolver's
prelude and fails if a name is missing from this file. It had drifted seven
milestones before that test existed.

## What it does not cover

No semantic knowledge: a capitalised name reads as a type by convention, not
because anything resolved it, and an undefined name looks like any other. That
is the language server's job, not a regex highlighter's.

One approximation: the lexer counts brace depth inside `${ }` properly, while
this file tracks one level of nesting. A record literal inside an interpolation
highlights correctly; a record literal inside a record literal inside an
interpolation does not.

## The icon

`plugin/skuld-icon.lua` gives `.skuld` files their own icon, so the file tree
(neo-tree, nvim-tree, snacks explorer) and the statusline (lualine) stop
drawing the default page. The glyph is the Nerd Font boxed S, U+F0B1A, in
purple. `assets/skuld.svg` is the same mark drawn as a logo: a letter cut in
straight strokes, the way a rune is cut, for anywhere a font glyph will not do.

It needs a Nerd Font in the terminal; without one the cell shows a box, and
nothing else changes. Two icon providers exist — `mini.icons`, which is what
LazyVim installs and what it makes `nvim-web-devicons` resolve to, and
`nvim-web-devicons` itself — and whichever is installed is registered. Neither
is required.

The timing is the whole of it. A provider is normally lazy, so it loads the
first time something asks it to draw — and that first request is the file tree
asking for this very icon, which is too late to be told about it. So the
provider is loaded on purpose at `VeryLazy` (`VimEnter` without LazyVim) and
extended in the same breath, before anything draws. Under a plugin manager the
`require` is what runs the provider's own `setup()`, which is why the
registration has to come after it rather than before.

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
files an open file imports; completion — variants after an enum name, exports
after an import qualifier, fields and methods after a value, and keywords, the
prelude and what is in scope otherwise; hover (`K`), which reports a
declaration the way it is written; go-to-definition (`gd`), which follows a
qualified name into the module that declared it, opening a file you never had
open; find-references (`grr`) and rename (`grn`), which reach every file the
editor has open and refuse a rename that would change which declaration a name
reaches; the outline (`gO`, and any breadcrumb or symbol picker), with a class's
fields and methods nested inside it; formatting (`vim.lsp.buf.format()`,
`gq` where `formatexpr` is set), which runs the same formatter `skuld fmt` does;
document highlights, which mark the other uses of the name the cursor rests on;
signature help while a call is being written, with the argument you are on
picked out; inlay hints (`vim.lsp.inlay_hint.enable(true)`), which write the
inferred type of a binding that does not declare one, on by default here
since a hint nobody turned on is a feature nobody has; and semantic tokens,
which recolour names from the checker over the syntax file underneath — a class
is not a struct, a `let` is not a `var`, and a method of the language is not a
name from this file.

Several of these answer quietly when they cannot. A file that does not parse
formats to no change rather than an error dialog, since the diagnostic already
says where the problem is; its outline falls back to the last text that parsed,
so it does not empty itself while a declaration is being typed. Inlay hints and
semantic tokens come from the last check that succeeded, so they go a moment
stale while a line is broken rather than disappearing, and a file that has
never checked has neither.

Signature help is the one that has to work on text that does not parse at all,
since a call is asked about while it is half-written. It finds the call by
reading the text — the innermost unclosed `(` before the cursor, and the commas
since — and looks the name up in the last good check. A cursor inside an array
literal or a lambda body is not in an argument list any more, and gets nothing
rather than the enclosing call's signature. Parameter names are those of the
declaration; a prelude binding such as `print` and a builtin method such as
`push` were never declared in a file, so theirs read as types.

A prelude binding such as `print` has a hover but no definition: it belongs to
the language, and there is nowhere to send you.

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
mkdir -p ~/.config/nvim/syntax ~/.config/nvim/ftdetect ~/.config/nvim/lsp ~/.config/nvim/plugin
ln -s ~/www/skuld-lang/editors/nvim/syntax/skuld.vim ~/.config/nvim/syntax/
ln -s ~/www/skuld-lang/editors/nvim/ftdetect/skuld.vim ~/.config/nvim/ftdetect/
ln -s ~/www/skuld-lang/editors/nvim/lsp/skuld.lua ~/.config/nvim/lsp/
ln -s ~/www/skuld-lang/editors/nvim/plugin/skuld-icon.lua ~/.config/nvim/plugin/
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
