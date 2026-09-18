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
| `runtime/strings.c` | The managed-memory runtime: retain, release, allocation, the checks that trap |
| `runtime/platform.c` | The platform layer: what the OS is called, behind names `std/` speaks. Compiled as its own translation unit, so its headers never reach the program |

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

## Deciding something about the language

`AGENTS.md` carries the rule: a decision about what Skuld is comes from agents
reaching agreement, over evidence, and not from whoever happened to be asked.
This is how that is done here rather than why.

**Before proposing a design, go and read what the languages you are about to
cite actually do.** Their source is public and their specifications are
written down. `WebFetch` and `WebSearch` are available for exactly this, and
so is reading a vendored copy if one is nearer. The failure this guards
against is not ignorance, it is a confident paraphrase: "Go does X" is worth
nothing in a design argument unless you opened `mksyscall_windows.go` and it
said X. Cite what you read, by file or by section, so the next reader can
check it instead of re-deriving it.

**Then have it argued against.** Launch a second agent and ask for the
strongest case *against* the design, not for a review of it. Give it the
evidence you gathered and the constraints from `AGENTS.md`, and ask what
would have to be true for the design to be wrong. When it disagrees, the
disagreement is the product — resolve it in writing before any code moves.

**Record the outcome where the decision lives**, which is `AGENTS.md` for a
language rule and `ROADMAP.md` for a milestone. Include what was rejected.
A reversal is worth writing plainly: "this reverses the earlier draft, and
here is what changed my mind" costs a sentence and saves the next agent from
reopening it.

**Verify a claim about behaviour by running the behaviour.** A milestone's
closing marker, a "supported" in `README.md`, a platform in a table — each is
a claim that some program does something somewhere. Run it there. Reading the
diff that asserts it is not the same check, and on a second platform it is
not a check at all.

## Committing

This repository uses micro commits: **one completed task, one commit.** Commit
as soon as a task is done and validated — do not let finished work pile up into
a single large change.

- Stage explicit paths. Never `git add -A`, `git add .`, or `git commit -a`.
- One concern per commit. A refactor riding along with a feature belongs in its
  own commit, as does a formatting pass.
- Match the existing log: `type(scope): imperative subject`, then a body saying
  **why** the change exists rather than listing the files. English, like the
  rest of the repository. Keep the trailer convention `git log` already shows.
- Another agent may be editing this tree at the same time. Before staging, sort
  by mtime and leave the recently-touched cluster alone; committing a file
  mid-edit captures a half-finished state. Scratch files at the repository root
  are not project content — leave them untracked.
- Validate before committing, not after: `cargo fmt --all`, the relevant tests,
  then `cargo clippy --workspace --all-targets -- -D warnings`. A commit whose
  tests were never run is worse than no commit.
- Push only when asked.
