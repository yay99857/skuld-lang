# Skuld for Visual Studio Code

Syntax highlighting and the language server, for `.skuld` and `.sk` files.

```
syntaxes/skuld.tmLanguage.json   the grammar
language-configuration.json      comments, brackets, indentation
extension.js                     starts the server and connects to it
server.js                        where the server is
package.json                     what registers them
```

The grammar colours a document on its own, with no process running. Everything
else comes from `skuld-lsp`: completion, hover, go to definition and
declaration, find references, rename, the outline, workspace symbols, signature
help, inlay hints, call hierarchy, folding, selection ranges, semantic tokens,
formatting, and the quick fixes the compiler attaches to its own diagnostics.

## Installing it locally

The extension is not published to a marketplace. Install the client library
once, then link the folder into VS Code's extension directory and restart:

```bash
cd editors/vscode && npm install
ln -s "$PWD" ~/.vscode/extensions/skuld
# VS Code OSS, VSCodium and Cursor read a different directory:
# ~/.vscode-oss/extensions, ~/.vscode-oss/extensions, ~/.cursor/extensions
```

On Windows, from a shell with permission to link:

```
cd editors\vscode && npm install
mklink /D "%USERPROFILE%\.vscode\extensions\skuld" "%CD%"
```

## Finding the server

In this order, and it stops at the first that answers:

1. `skuld.server.path`, if it is set — someone who has said where the server is
   has said so for a reason.
2. `skuld-lsp` on `PATH`.
3. This checkout's `target/release/skuld-lsp`, then `target/debug`. Release
   first: someone who ran `cargo build --release` meant to use it.

So `cargo build --release` in the repository root is enough, with nothing
installed anywhere. If none of the three answers, the extension says so once
and highlighting carries on working — the alternative is silence, which reads
as the feature not existing.

`../nvim/lsp/skuld.lua` answers the same question the same way. The two should
not drift.

## A note on the project root

A Skuld program has no manifest. The root is the directory the entry file lives
in, which is how the compiler resolves every import path, so there is no project
file to look for and no workspace shape to require. The server reads each open
document as the entry file of its own program.

## What the grammar covers

Keywords, the primitive and builtin types, the prelude bindings, builtin
methods, declarations (the name a `func`, `class`, `struct`, `enum`, `const`,
`static` or `let` introduces), comments, numbers in all four bases with `_`
separators, strings with `${ }` interpolation highlighted as the expressions
they are, char literals, and the operators.

Everything there is taken from the compiler rather than from the roadmap, and
`cli/tests/editors.rs` fails when the two disagree — the same test also checks
that this extension still starts the server it says it starts.
