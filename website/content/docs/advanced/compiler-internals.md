---
title: Compiler internals
description: Follow source through the compiler's separate semantic and native stages.
status: Implemented
---
The compiler is written in Rust. Its Cargo workspace contains the compiler library, command-line tool and language server.

## Pipeline

```text
Source → Lexer → Parser → AST → Resolver
       → Type checker → HIR → C → clang → Native executable
```

Parsing uses handwritten recursive descent and Pratt expression parsing. Name resolution and checking are separate from syntax. Only successful checking constructs a typed program; a separate lowering pass constructs backend HIR.

## Source locations

Tokens and nodes retain half-open byte spans. Resolver tables are tied to their exact AST revision and keyed by file and offset. Diagnostics render these spans as source locations.

## Runtime boundary

The code generator reads HIR. Managed ownership helpers live in `runtime/strings.c`; OS bridges live in `runtime/platform.c`, compiled separately. File loading and external processes belong to the CLI, not the compiler library.

## Inspection commands

Use `lex`, `parse` and `resolve` to inspect stages. Use `check` for full static checking and `emit-c` for checked backend output. Debug AST output is not a stable serialization format.

## Contributing

Read the repository's [AGENTS.md](https://github.com/yay99857/skuld-lang/blob/main/AGENTS.md), [LANGUAGE.md](https://github.com/yay99857/skuld-lang/blob/main/LANGUAGE.md) and [ROADMAP.md](https://github.com/yay99857/skuld-lang/blob/main/ROADMAP.md). A planned milestone is not an implemented language feature.
