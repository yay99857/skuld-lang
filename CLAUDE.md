# CLAUDE.md

Guidance for Claude Code when working in this repository.

@AGENTS.md

## Project in one paragraph

Skuld is an experimental, statically typed, native language implemented as a
Rust workspace (edition 2024, stable toolchain, no external crate dependencies).
`compiler/` is the `skuld-compiler` library; `cli/` is the `skuld-cli` package
that builds the `skuld` binary. The pipeline is
`source → lexer → parser → AST → resolver → type checker → HIR → C → clang`.

## Layout

| Path | Contents |
| --- | --- |
| `compiler/src/span.rs`, `diagnostic.rs`, `token.rs` | Byte spans, `SourceFile`, diagnostic rendering, token kinds |
| `compiler/src/lexer.rs`, `parser.rs`, `ast.rs` | Recursive descent + Pratt expressions into a token-independent AST |
| `compiler/src/resolver.rs` | Lexical value-name resolution tables |
| `compiler/src/types.rs`, `type_checker.rs` | Semantic type enums and the typed program |
| `compiler/src/hir.rs`, `lowering.rs`, `codegen_c.rs` | HIR lowering and C emission |
| `compiler/src/lib.rs` | Stage re-exports plus `check()` and `compile_to_c()` |
| `cli/src/main.rs`, `cli/tests/cli.rs` | Argument handling, rendering, integration tests |
| `cli/src/native.rs` | clang orchestration; the compiler library never launches processes |
| `examples/` | `hello.skuld`, `functions.skuld` — the Demo 0/1 sources |
| `tests/pass|fail|trap/` | Language golden fixtures, run by `cli/tests/golden.rs` (see `tests/README.md`) |
| `runtime/` | Placeholder; keep it minimal until allocation or reference management needs it |

Unit tests live in `compiler/src/<stage>/tests.rs` next to the stage they cover.

## Checking the real status before you write

`AGENTS.md` carries the authoritative rules, but its status section can lag
behind the code. Verify the current surface before describing it to the user or
editing docs:

```bash
grep -n 'pub mod\|pub fn' compiler/src/lib.rs      # stages actually wired up
grep -n '"lex"\|"parse"\|"resolve"' cli/src/main.rs # subcommands actually exposed
```

A module existing in `compiler/src/` does not mean the CLI exposes it, and a
command named in `README.md` under "future" may already have a library entry
point. State what you verified, not what the prose claims.

## Commands

```bash
cargo build
cargo run -p skuld-cli -- lex examples/hello.skuld
cargo run -p skuld-cli -- parse examples/functions.skuld
cargo run -p skuld-cli -- resolve examples/functions.skuld
```

Before finishing any code change:

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Run a single stage's tests with `cargo test -p skuld-compiler parser::tests`, and
the language golden fixtures with `cargo test -p skuld-cli --test golden`.
When another agent is building concurrently, point `CARGO_TARGET_DIR` at a
scratch path so the two do not fight over the `target/` lock.
Documentation-only edits need a consistency review across `README.md`,
`LANGUAGE.md` and `AGENTS.md`, not a full Rust run.

## Working notes

- Keep the stages separate. Never emit C from the AST or skip semantic analysis
  to make an example run.
- Invalid user source produces diagnostics; panics mean a compiler bug. Avoid
  `unwrap()`/`expect()` on user-input paths — they are fine in tests.
- Preserve half-open byte spans through lowering; derive line/column from
  `SourceFile`, never store them.
- `README.md` and `LANGUAGE.md` distinguish Implemented / Planned /
  Experimental. Changing behavior means updating those markers in the same
  change; documenting a feature is not permission to implement it.
- Stay inside the active milestone described in `AGENTS.md`.
