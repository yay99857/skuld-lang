---
title: Editor support
description: Use Skuld syntax highlighting and the language server in your editor.
status: Implemented
---
The repository includes `skuld-lsp`, a VS Code extension and a Neovim configuration. The VS Code extension is installed locally; it is not published to a marketplace.

## Build the language server

From the compiler repository:

```bash
cargo build --release
```

The extension looks for an explicit `skuld.server.path`, then `skuld-lsp` on PATH, then the repository's release and debug outputs.

## Install the VS Code extension

Install its client dependency from `editors/vscode` with `npm install`, then link that directory into your editor's extension directory and restart. The exact directory differs between VS Code, VSCodium and Cursor.

Follow the repository's [editor installation instructions](https://github.com/yay99857/skuld-lang/blob/main/editors/vscode/README.md) for Linux and Windows linking commands. Windows directory links require the appropriate shell permissions.

## Language features

The server provides diagnostics, completion, hover, definitions, references, rename, document symbols, signature help, inlay hints, semantic tokens, formatting, call hierarchy and supported quick fixes.

## Rename boundaries

Functions, parameters and locals can be renamed. Types, fields, methods, import qualifiers, prelude bindings and embedded library names are outside the current rename support. Renames are rechecked to avoid introducing name capture.

The server treats open documents as program entries. References in programs that no open document reaches are not part of that workspace view.
