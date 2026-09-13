# Working on Skuld

These instructions apply to the entire repository. Follow the user's current
task and keep changes within its milestone. Read `LANGUAGE.md` for the living
specification and `README.md` for usage and current capabilities before changing
language behavior. `ROADMAP.md` records the proposed ordering of future
milestones. Keep their Implemented / Planned / Experimental distinctions
accurate; documenting a future feature is not a request to implement it.

## User syntax reference

- Read the root [test.skuld](test.skuld) before proposing or changing language
  syntax. The user maintains demonstrations there to communicate the desired
  direction; consult the current file rather than copying a frozen example
  into this document.
- Treat it as a living syntax reference, not an executable test or proof that
  a feature is implemented. The current sketches include classes, methods
  without a `func` prefix, `this`, `new`, arrays and sorting callbacks.
- The implemented class syntax in `LANGUAGE.md` adopts `hello()` methods,
  implicit `this` and `new User(...)` construction. Top-level functions retain
  `func`. Do not restore the older class `func greet(self)` or
  `User { ... }` syntax. User-defined constructors and field defaults still need design;
  current construction requires every named field;
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
- Prefer composition over inheritance. Structs have value/copy semantics;
  classes have reference semantics with reference counting. Do not introduce
  inheritance, GC, a borrow checker or Rust's ownership system.
- No normal `null` value. Optional values use builtin `Option<T>`; future errors
  use `Result`. Exceptions are not the primary error mechanism.

## Repository and current milestone

- Rust stable, edition 2024; one Cargo workspace with two crates:
  `compiler/` is the `skuld-compiler` library, and `cli/` is the `skuld-cli`
  package exposing the `skuld` binary. There are currently no external crate
  dependencies. Add dependencies only for a concrete need.
- Implemented: the entire source → lexer → parser → AST → resolver → type
  checker → HIR → C → clang pipeline, native Demos 0–3 and loops. Scalar types
  are int, float, bool, string and void; functions, locals, calls, returns,
  conditionals, `while`, `loop`, `break`, `continue`, structs, classes and
  reference-counted string concatenation and interpolation execute. Weak class
  references, homogeneous arrays, builtin Option values and builtin `Result<T, E>`
  with `?` propagation execute too. Parameters and `let` bindings are immutable.
- `lex`, `parse`, `resolve` inspect individual stages. `check` performs full
  static checking without clang; `emit-c` emits checked C; `run` builds and
  executes in a private temporary directory, keeping nothing; `build` keeps the
  executable in the working directory under the source stem.
- The resolver uses single-source declaration/use tables; keep them with their
  exact AST revision. Functions are predeclared; parameters share the function
  body scope; locals become visible after initializers; child scopes shadow.
  The `print`, `Some` and `None` prelude bindings may be shadowed. Use resolved symbols, not
  spelling, to identify builtins. Only direct calls are supported currently.
- HIR lowering is separate from checking. Only successful checking constructs
  a TypedProgram, and only lowering constructs backend HIR. Do not expose
  mutation that can invalidate these invariants.
- Preserve left-to-right evaluation and short-circuit boolean operations.
  Compound assignments snapshot the old value before the RHS. Integer overflow
  and invalid integer division trap; never introduce signed C overflow UB.
  Strings are length-aware views of static literal bytes, including NUL.
- Structs are implemented with value/copy semantics: fields, record
  construction, field access and field assignment through places. Methods are
  declared without `func`, take an implicit immutable `this`, and lower to
  functions with a leading receiver. Struct names live in a type namespace
  owned by the checker, and method names in a scope of their own, so neither
  resolves as an ordinary value name.
- Classes are implemented with reference semantics: fields, methods with implicit
  `this`, `new Class(...)` construction, field access and field assignment through
  references. Assigning into a class field is allowed on `let` bindings and through
  `this`. Heap allocation is reference counted.
- Memory is reference counted with non-atomic counts, no garbage collector and
  no cycle collector. The runtime is `runtime/strings.c`, embedded verbatim in
  generated C; do not restate retain/release in the code generator. Ownership
  is emitted with cleanup attributes: fresh values are adopted, borrowed values
  retained on entry, arguments borrowed, returns retained.
- Weak class references use `weak Class`, `weak(value)` and contextually typed
  empty `weak()`. `upgrade()` returns an owning `Option<Class>` without trapping
  on expiration; `alive()` checks liveness and `get()` retains or traps.
  Weak references do not keep managed fields alive. No normal null value exists.
- Builtin `Option<T>` uses inline tag/payload value semantics. Contextual `null` (and
  `None`) represents absent values; values wrap implicitly into expected Options.
  `if let name = value` (and `if let Some(name)`) binds an immutable payload in its then scope.
  Managed payloads retain/release only when present. General generics and
  interfaces remain future work.
- Builtin `Result<T, E>` uses the same inline tag/payload layout as an enum: `Ok`
  is tag 0 and `Err` tag 1, so matching, retain and release are shared code.
  `Ok(v)`/`Err(e)` require an expected Result type — neither side is inferred
  from the other — and no bare value wraps implicitly. `match`, `if let Ok(x)`,
  `if let Err(e)`, `is_ok()` and `is_err()` inspect a Result. Postfix `?` yields
  the success payload or returns the error unchanged; it requires an enclosing
  function returning a Result with an identical error type and never converts
  between error types. `Result` is a reserved type name; `Ok` and `Err` are
  shadowable prelude bindings.
- Arrays use `[]T`, literals, checked int indexes, `len()`, `push()`, `insert()`,
  `pop()` and `remove()`. Capacity grows geometrically; references share element
  mutations even through `let`. Managed elements are retained and released. Slicing,
  sorting and callbacks remain future work.
- Enums are user-declared sum types with unit and payload variants: `enum Name { Variant, Variant(Type) }`.
  Pattern matching uses `match value { Pattern: stmt, Pattern: { ... }, _: ... }` with exhaustiveness
  checking, immutable payload arm bindings, and C codegen retaining/releasing managed variant payloads.
- `for` loops iterate over half-open integer ranges `a..b` and arrays `[]T` by value:
  `for i in 0..10 { ... }` and `for item in items { ... }`. Loop variables are immutable
  and scoped to the body. Managed array elements retain and release per iteration.
  `break` and `continue` naturally bind to the loop.
- Completed: classes, weak references, dynamic arrays (push, insert, pop, remove),
  colon return type syntax, Option with null, safe weak promotion, M1 (Enums and match),
  M2 (`for` and iteration), and M3 (`Result<T, E>` and propagation), which the user
  authorized as a builtin following the Option precedent rather than through general
  generics. The next milestone would be M4 (bytes, sized integers and string slices);
  do not infer authorization for further features without explicit decision.
- `ROADMAP.md` proposes the sequence enums/`match` → `for` → `Result` → bytes
  and string slices → `extern "C"` FFI, with modules and networking beyond it.
  It is a plan, not a selection: a remaining entry is Planned, and starting one
  still requires an explicit decision recorded here. Its `Result`-versus-generics
  question was answered in favour of a builtin; the rest (slice retention,
  recursive enum variants, callback syntax, JSON object representation) are
  unresolved; do not settle them unilaterally while implementing something else.
- Compiler unit tests live beside modules; CLI/native tests are in `cli/tests/`.
  Root `tests/pass`, `tests/fail` and `tests/trap` contain language fixtures.
  Full workspace testing requires clang: every `tests/pass` fixture is built
  with address, leak and UB detection, so a leak or double free fails the suite.
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
before Demo 3. Interfaces, FFI and the official formatter remain future work
unless explicitly included in the active task.
Do not build a standard library or memory-management runtime ahead of need.

The milestones proposed in `ROADMAP.md` do not relax any of the above.
Sized integers, `[]u8`, string slices, `extern "C"`, modules, `import`, HTTP and
JSON are all Planned and each needs its own authorization. `Result` arrived as a
builtin, which settles that roadmap question; general generics remain excluded
and still need their own explicit decision. Nothing in that document
authorizes a standard library, a networking runtime or process execution from
the compiler library.

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
