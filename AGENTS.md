# Working on Skuld

These instructions apply to the entire repository. Follow the user's current
task and keep changes within its milestone. Read `LANGUAGE.md` for the living
specification and `README.md` for usage and current capabilities before changing
language behavior. Keep their Implemented / Planned / Experimental distinctions
accurate; documenting a future feature is not a request to implement it.

## User syntax reference

- Read the root [test.skuld](test.skuld) before proposing or changing language
  syntax. The user maintains demonstrations there to communicate the desired
  direction; consult the current file rather than copying a frozen example
  into this document.
- Treat it as a living syntax reference, not an executable test or proof that
  a feature is implemented. The current sketches include classes, methods
  without a `func` prefix, `this`, `new`, arrays and sorting callbacks.
- The updated planned class syntax in `LANGUAGE.md` adopts `hello()` methods,
  implicit `this` and `new User(...)` construction. Top-level functions retain
  `func`. Do not restore the older class `func greet(self)` or
  `User { ... }` syntax. Constructors and field initialization still need design;
  global statements and sorting callbacks remain experimental proposals.
- Read comments as design feedback. Examples marked for revision are not
  settled syntax. An `undefined` output comment does not establish an undefined
  value in Skuld or override its strong typing and no-null design.
- When a sketch differs from `LANGUAGE.md` or current behavior (for example
  construction syntax, receiver spelling or global statements), identify the
  difference when working on that feature. Follow explicit user decisions and
  update the specification and tests as the design is settled; do not silently
  treat all sketches as implemented or expand the active milestone.
- Preserve the user's demonstrations. Do not rewrite, format or convert this
  file into a passing fixture unless the task asks for that change. Maintain
  executable regression examples separately in `examples/` and `tests/`.

## Identity and priorities

- Skuld is an independent, experimental, statically typed native language.
  TypeScript is only a readability reference alongside Go, V and Rust. Do not
  turn Skuld into a syntax copy, dialect or second TypeScript. Compatibility
  with TypeScript/JavaScript source, type systems or runtimes is not a goal.
- Preserve `func`, `->`, immutable `let` and mutable `var`. Use `.skuld` files.
  Do not replace these choices merely to resemble another language.
- Prioritize simplicity, predictability, strong static typing, local inference,
  explicitly typed function signatures and actionable diagnostics.
- Target native performance in the Go/Rust range. Treat this as a goal requiring
  comparable benchmarks, not a guarantee provided by the C backend.
- Initial semantic numeric aliases are platform-independent `int = i64` and
  `float = f64`. Avoid implicit coercions. Lexical integer magnitudes use `u64`
  so the signed minimum can later be handled correctly by semantic analysis.
- Prefer composition over inheritance. Structs will have value/copy semantics;
  classes will have reference semantics with future ARC. Do not introduce
  inheritance, GC, a borrow checker or Rust's ownership system.
- No normal `null` value. Future optional values and errors use `Option` and
  `Result`; exceptions are not the primary error mechanism.

## Repository and current milestone

- Rust stable, edition 2024; one Cargo workspace with two crates:
  `compiler/` is the `skuld-compiler` library, and `cli/` is the `skuld-cli`
  package exposing the `skuld` binary. There are currently no external crate
  dependencies. Add dependencies only for a concrete need.
- Implemented: the entire source → lexer → parser → AST → resolver → type
  checker → HIR → C → clang pipeline, native Demos 0–2 and loops. Scalar types
  are int, float, bool, string and void; functions, locals, calls, returns,
  conditionals, `while`, `loop`, `break` and `continue` execute. Parameters and `let` bindings are immutable.
- `lex`, `parse`, `resolve` inspect individual stages. `check` performs full
  static checking without clang; `emit-c` emits checked C; `run` builds and
  executes in a private temporary directory. `build` remains unimplemented.
- The resolver uses single-source declaration/use tables; keep them with their
  exact AST revision. Functions are predeclared; parameters share the function
  body scope; locals become visible after initializers; child scopes shadow.
  The `print` prelude binding may be shadowed. Use resolved symbols, not
  spelling, to identify builtins. Only direct calls are supported currently.
- HIR lowering is separate from checking. Only successful checking constructs
  a TypedProgram, and only lowering constructs backend HIR. Do not expose
  mutation that can invalidate these invariants.
- Preserve left-to-right evaluation and short-circuit boolean operations.
  Compound assignments snapshot the old value before the RHS. Integer overflow
  and invalid integer division trap; never introduce signed C overflow UB.
  Strings are length-aware views of static literal bytes, including NUL.
- Next milestone: the planned Demo 3 showcase. Members
  are parser syntax only and are rejected by checking until their milestone.
- Compiler unit tests live beside modules; CLI/native tests are in `cli/tests/`.
  Root `tests/pass`, `tests/fail` and `tests/trap` contain language fixtures.
  Full workspace testing requires clang (including UBSan for a native test).
  `runtime/` stays minimal until managed allocation needs it.
- Report status as completed milestones and verified behavior. Do not introduce
  completion percentages or progress scores; they imply a precision the project
  cannot justify and drift out of sync across documents.
- Update this status section when an authorized milestone is completed.

## Architecture rules

The implemented pipeline is:

```text
source → lexer → parser → AST → resolver → type checker → HIR
       → C backend → clang → native executable
```

- Keep lexing, parsing, name resolution, type checking and generation separate.
  Never emit C while parsing or bypass semantic stages to demonstrate execution.
- Use handwritten recursive descent for declarations/statements and Pratt
  parsing for expressions. Do not add parser generators.
- Keep AST types independent of lexer token kinds. Source type names belong in
  syntax; resolved semantic types must use enums/IDs, not magic strings.
- Preserve half-open byte spans on tokens and relevant nodes through semantic
  analysis and lowering. Derive filenames, lines and columns from source data.
- Generate C from a simple HIR, not directly from AST or tokens. Use prefixed
  generated names to avoid collisions. Do not introduce a complex MIR now.
- Preserve documented statement-boundary rules, precedence and associativity.
  Newlines are not lexer tokens; expressions can continue across lines.
  Add regression tests when changing these rules rather than inserting a
  simplistic newline-to-semicolon transformation.
- Keep code explicit and readable. Do not split into more crates or add generic
  backend/runtime abstractions without a demonstrated architectural need.

## Milestone discipline

Complete the resolver, type checker, HIR and C backend before claiming native
Demo 0. Demo 0 must execute hello through the entire pipeline using clang.
Demo 1 adds the working function/variable example printing `42`; Demo 2 adds
conditional flow. Loops follow Demo 2. Classes, fields, methods, construction,
implicit `this` and string interpolation belong to Demo 3, after functions and type
checking are stable.

Do not implement generics, macros, async/await, threads, channels, reflection,
decorators, annotations, a package registry, compiler plugins, compile-time
execution, operator overloading, user-defined conversions, LLVM or Cranelift
before Demo 3. Interfaces, enums, arrays, Option/Result, FFI, ARC and the official
formatter remain future work unless explicitly included in the active task.
Do not build a standard library or memory-management runtime ahead of need.

## Diagnostics and validation

- Invalid user source must produce diagnostics, not panic. Avoid `unwrap()` and
  `expect()` in user-input paths; assertions in tests are acceptable. Panics
  may only indicate internal compiler bugs.
- Diagnostics carry a typed code, meaningful message, span and optional help.
  Preserve useful recovery and the existing no-partial-AST-on-error contract.
  Guard pathological nesting without losing source locations.
- Test behavior: precedence, spans, multiline rules, valid examples, invalid
  input, error recovery and CLI exit/output behavior. Add semantic and backend
  tests as those stages arrive. Do not write tests that only mirror internals.
- For code changes, format and run the relevant tests, then the workspace checks:

```bash
cargo fmt --all
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

- Smoke-test affected CLI paths when relevant:

```bash
cargo run -p skuld-cli -- lex examples/hello.skuld
cargo run -p skuld-cli -- parse examples/functions.skuld
cargo run -p skuld-cli -- resolve examples/functions.skuld
cargo run -p skuld-cli -- check examples/functions.skuld
cargo run -p skuld-cli -- run examples/hello.skuld
cargo run -p skuld-cli -- run examples/functions.skuld
cargo run -p skuld-cli -- run examples/conditionals.skuld
```

- Documentation-only edits need a consistency review, not a full Rust test run.
  Keep examples and command instructions honest about what currently works.
- Report the changes, validation actually executed, and remaining limitations.
  Never claim a check passed without running it or call parsing a type check.
