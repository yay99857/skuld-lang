# Skuld for Visual Studio Code

Syntax highlighting for `.skuld` and `.sk` files.

```
syntaxes/skuld.tmLanguage.json   the grammar
language-configuration.json      comments, brackets, indentation
package.json                     what registers them
```

## Installing it locally

The extension is not published to a marketplace. To use it from this
repository, link it into VS Code's extension directory and restart:

```bash
ln -s "$PWD/editors/vscode" ~/.vscode/extensions/skuld
# VS Code OSS, VSCodium and Cursor read a different directory:
# ~/.vscode-oss/extensions, ~/.vscode-oss/extensions, ~/.cursor/extensions
```

## What it covers

Keywords, the primitive and builtin types, the prelude bindings, builtin
methods, declarations (the name a `func`, `class`, `struct`, `enum`, `const`,
`static` or `let` introduces), comments, numbers in all four bases with `_`
separators, strings with `${ }` interpolation highlighted as the expressions
they are, char literals, and the operators.

Everything here is taken from the compiler rather than from the roadmap, and
`cli/tests/editors.rs` is what keeps it that way: it reads the keyword table
out of `compiler/src/lexer.rs` and the prelude out of
`compiler/src/resolver.rs`, and fails when a name the compiler knows is
missing from this grammar.

## What it does not cover

A grammar is a regular expression over lines, so it approximates what the
compiler decides properly:

- `union`, `packed` and `align` are keywords in one position each and ordinary
  identifiers everywhere else. `union` is coloured only after `extern`; the
  other two only before a body.
- A capitalised name is highlighted as a type, which is convention rather than
  a rule — the compiler enforces no case.
- A prelude binding is shadowable, so a local named `print` is still coloured
  as the builtin.
- Nested braces inside `${ ... }` end the interpolation early. The lexer counts
  depth; this does not.

The editor knowledge that cannot be approximated — what a name resolves to,
what type an expression has, where a declaration is — comes from `skuld-lsp`
instead, which speaks the same protocol to any editor.

## The language's own colour

`.github/linguist/skuld.yml` carries the entry this project proposes to
GitHub's Linguist, which is where a language's name and colour are decided.
A grammar is one of the two things that entry needs; the other is evidence
that people write `.skuld` files.
