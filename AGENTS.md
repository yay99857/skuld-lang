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
- Semantic numeric types are platform-independent: `int = i64`, `float = f64`,
  and the sized integers `i8 i16 i32 i64` / `u8 u16 u32 u64`. Avoid implicit
  coercions, between widths included. Lexical integer magnitudes use `u64` so
  the signed minimum is handled correctly by semantic analysis.
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
  references, homogeneous arrays, builtin Option values, builtin `Result<T, E>`
  with `?` propagation, the sized integers, byte-level string access and
  `extern "C"` foreign calls, programs made of several modules and the embedded
  standard library execute
  too. Parameters and `let` bindings are immutable.
- `lex`, `parse`, `resolve` inspect individual stages. `check` performs full
  static checking without clang; `emit-c` emits checked C; `run` builds and
  executes in a private temporary directory, keeping nothing; `build` keeps the
  executable, under the source stem in the working directory or wherever `-o`
  names, and never overwrites its own source. `build` and `run` also forward
  `-l<library>`/`-L<directory>` to clang and accept no other linker argument, so
  nothing there can redirect the output or change how the program is compiled;
  `-o` may point anywhere the user can write, since it carries no code into the
  build, unlike a module path. Argument parsing is one pure function with its
  own tests: options on either side of the file, `--` to end them, `-h/--help`
  and `-V/--version` answered before anything else is judged and printed to
  stdout, a misuse on stderr with exit 2, and a mistyped command matched against
  the real ones with a transposition-aware distance.
- The resolver's declaration and use tables are keyed by file and byte offset;
  keep them with their exact AST revision. Functions are predeclared; parameters share the function
  body scope; locals become visible after initializers; child scopes shadow.
  The prelude bindings — `print`, `Some`, `None`, `null`, `Ok`, `Err`, `ptr`, the width
  conversions and `bytes_to_string` — may all be shadowed. Use resolved symbols,
  not spelling, to identify builtins. Only direct calls are supported currently.
- HIR lowering is separate from checking. Only successful checking constructs
  a TypedProgram, and only lowering constructs backend HIR. Do not expose
  mutation that can invalidate these invariants.
- Preserve left-to-right evaluation and short-circuit boolean operations.
  Compound assignments snapshot the old value before the RHS. Integer overflow,
  invalid integer division and out-of-range width conversion trap at every
  width; never introduce signed C overflow UB. Strings are length-aware views of
  static literal bytes, including NUL; a slice of owned bytes copies, while a
  slice of literal bytes stays a view.
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
  `pop()`, `remove()` and `[a..b]` slicing. Capacity grows geometrically; references
  share element mutations even through `let`. Managed elements are retained and
  released. Sorting and callbacks remain future work.
- Sized integers are `i8 i16 i32 i64` and `u8 u16 u32 u64`; `int` is a spelling of
  `i64`, not a separate type. A literal takes the width its context expects and is
  range-checked there, defaulting to `int`; a signed minimum is written as a minus
  on a literal. Widths never mix implicitly. `u8(v)`, `int(v)` and the rest are
  explicit conversions that trap out of range, are prelude bindings like `print`,
  and carry their width on the resolved symbol rather than in the spelling.
  Arithmetic traps on overflow and invalid division at every width; unary `-` is
  rejected on unsigned types. Integer/float conversion is not implemented.
- Strings are byte sequences: `len()` counts bytes, `text[i]` reads a `u8`,
  `text[a..b]` slices, `text.bytes()` yields `[]u8`, and strings stay immutable,
  so an indexed write is rejected. Slices copy rather than retaining their source,
  except that slicing a literal is a view, since literal bytes are static.
  `bytes_to_string([]u8) -> Result<string, string>` validates strict UTF-8; its
  string error side is provisional and awaits a standard library to own a real
  error type.
- Enums are user-declared sum types with unit and payload variants: `enum Name { Variant, Variant(Type) }`.
  Pattern matching uses `match value { Pattern: stmt, Pattern: { ... }, _: ... }` with exhaustiveness
  checking, immutable payload arm bindings, and C codegen retaining/releasing managed variant payloads.
- `for` loops iterate over half-open integer ranges `a..b` and arrays `[]T` by value:
  `for i in 0..10 { ... }` and `for item in items { ... }`. Loop variables are immutable
  and scoped to the body. Managed array elements retain and release per iteration.
  `break` and `continue` naturally bind to the loop.
- Foreign functions are declared in `unsafe extern "C" { func name(...) -> type }`
  blocks with no bodies. The `unsafe` marker is required and sits on the block,
  because the unsafe act is asserting a signature the compiler cannot verify
  against the linked library; the calls themselves are ordinary direct calls,
  resolved and checked like any other, and the backend emits the declared name
  verbatim instead of a generated one. Only `"C"` is a supported ABI.
  Managed values never cross the boundary, and the checker enforces it: a
  signature accepts only the scalars, the raw pointer type `*T` over a scalar or
  `*void`, and `void` as a return type; a string, array, class, Option, Result
  or a pointer to one is rejected. A declared name may not be `main` or start
  with `skuld_`. `ptr(value)` borrows the bytes a string or an array of scalars
  already owns — `*u8` for a string, `*T` for a `[]T` — retains nothing and is
  valid only while its operand is alive, so a foreign call emits no retain,
  release or cleanup of its own. Length travels separately; a Skuld string is
  not NUL-terminated. Libraries are named on the command line, never in the
  source. Callbacks into C, structs by value across the ABI, varargs, pointer
  arithmetic and reading through a pointer from Skuld remain out of scope.
- Completed: classes, weak references, dynamic arrays (push, insert, pop, remove),
  colon return type syntax, Option with null, safe weak promotion, M1 (Enums and match),
  M2 (`for` and iteration), M5 (`extern "C"` FFI and linking, authorized by the
  user in the session that implemented it), M7 (a minimal standard library,
  authorized by the same user decision that settled where `std` is rooted), and M3 (`Result<T, E>` and propagation), which the user
  authorized as a builtin following the Option precedent rather than through general
  generics. M4 (bytes, sized integers and string slices) is complete — the user
  authorized the full set of sized integers, type-name conversion calls, and
  delivering the primitives before the milestone's closing marker, and that
  marker is now met: `tests/pass/json_parser.skuld` is a complete
  JSON parser in pure Skuld over `[]u8`, so M4 is closed. The rough edge it
  exposed is fixed: a conversion's expected width reaches an integer literal,
  possibly signed or parenthesised, and stops there, so `u8(128 + n % 64)`
  compiles and a computed argument is checked when it is converted. M6 (modules and `import`) is complete, by explicit user
  decision on its three open questions: a module is a directory whose `.skuld`
  files share a namespace, export is an explicit `pub`, imported names are
  always qualified by the module's last path segment, and an import cycle is a
  diagnostic. Its rules are set out in their own entry below, as are M7's. M6 was selected
  without waiting for M5, which modules do not depend on; M5 landed
  concurrently in another session (`2606206`, `e3d84c2`) and owns its own
  status entry. M8 (function values and lambdas) is complete; its three open
  design questions were delegated by the user and are recorded in `ROADMAP.md`.
  M9 (sockets, HTTP and the long-range target) is complete: `std/net` is a
  blocking TCP connection over libc, `std/http` an HTTP/1.1 client on top of
  it, and `std/json` is M4's parser promoted out of its fixture. A Skuld
  program fetches a document and decodes it. No milestone is active. Do not
  infer authorization for further features without explicit decision.
- `ROADMAP.md` proposes the sequence enums/`match` → `for` → `Result` → bytes
  and string slices → `extern "C"` FFI → modules → standard library → function
  values → sockets. It is a plan, not a selection: a remaining entry is Planned,
  and starting one still requires an explicit decision recorded here, and the
  sequence may be taken out of order, as M6 was. Its `Result`-versus-generics,
  slice-retention, JSON-object and module-boundary questions are answered there;
  recursive enum boxing, callback syntax, capture and where interfaces belong
  are not. Do not settle those unilaterally while implementing something else.
- Modules are implemented: a module is a directory whose `.skuld` files share one
  namespace; `import "path"` precedes every declaration, resolves against the
  program root — the entry file's directory — and binds the path's last segment
  as the only way to name that module's exports. Path segments are plain names,
  so a path cannot climb out of the root. `pub` exports a function, struct, class
  or enum; a method is public with the type that owns it and an `extern` block
  cannot be exported. Imports belong to a file, declarations to its module.
  Scopes nest prelude → module → file → bodies, and a local binding shadows a
  qualifier. An import cycle, a duplicate qualifier in one file, a private name
  and a missing module are diagnostics. The compiler performs no I/O: the graph
  asks a `ModuleLoader` the caller supplies, and the CLI's reads directories.
- The standard library is implemented: `std` is a reserved import prefix whose
  modules are written in Skuld under `std/` and embedded in the compiler binary
  by `compiler/src/std_lib.rs`. A reserved path is intercepted in
  `Loader::module_for` and never reaches the caller's `ModuleLoader`, so a
  directory named `std` is unreachable — keep that reservation in the compiler,
  since it is a language rule and not a filesystem one — and a reserved path
  naming no module is an error listing the ones that exist, never a fallback to
  disk. The library is `std/utf8` (a real `Utf8Error` with byte offsets, strict
  `validate`, `decode`, `count`, `describe`), `std/strings` (`starts_with`,
  `ends_with`, `index_of`, `contains`, `trim`, `split`, `join`, all in byte
  offsets) and `std/cstring` (`to_c`, refusing an embedded NUL). It stays small
  on purpose: every entry needs a caller in a milestone already planned, and
  adding one means changing the compiler, which is the intended brake. The
  builtin `bytes_to_string` keeps returning `Result<string, string>`: making it
  return a library type would invert the dependency.
- Function values are implemented and non-escaping. A lambda is written
  `(a: int, b: int): int { ... }` — no keyword, the shape a method already uses
  — and a function type is `(int, int) -> int`, which keeps `->` so a parameter
  is not spelled `compare: (int, int): int`. Parameter and result types may be
  omitted where the expected type supplies them, and a declared function named
  where a value is expected becomes one. A function value may be a parameter or
  a local and nothing else: never a field, a return type, an array element or a
  payload. That rule is the safety argument, not a detail — relaxing it
  reintroduces cycles the reference counter cannot collect — and it is what
  makes a function value free: no allocation, no retain, no release, with
  captures copied into an environment in the enclosing block. Only immutable
  bindings may be captured, since a copy of a `var` could go stale. A
  consequence to keep written down: a callback cannot be stored, so handler
  tables and registries wait for interfaces. A lambda body must open on the
  same line its parentheses close, or `var x = (None)` followed by a block would
  read as one. `sort()` is in place, returns void, is stable, and runs on a
  snapshot so a comparator that mutates the array aborts instead of reading a
  reallocated buffer.
- A declaration can unwrap: `let name = value else binding { ... }` binds the
  payload of an `Option` or a `Result` for the rest of the scope and runs the
  block when there is none. A `Result` names its error there; an `Option` has
  nothing to name, and writing a name is an error. The block must not fall
  through, because the name outlives the statement — `return`, `break` and
  `continue` leave, and an `if` that only sometimes returns does not. It relaxes
  neither `?`'s refusal to convert error types nor `main` returning `void`; it
  exists so a failure can be handled without nesting. Added after the user found
  the nested `match` at a call site too Rust-shaped; the Go model was considered
  and is blocked by Skuld having no zero values, which is a decision worth
  keeping.
- The standard library reaches the network, with three limits that are
  structural rather than unfinished, each following from pointer reads being out
  of scope at the foreign boundary. There is no name resolution — every resolver
  in libc answers with a pointer to a structure — so a connection is made to an
  IPv4 address and `http.get` refuses a name by name. There is no `errno`, so a
  network failure says which step failed, not why. There is no TLS, so `https://`
  is refused rather than attempted; binding one is a milestone of its own and
  arguably a dependency-policy decision. Do not close any of these three without
  an explicit decision: the alternatives are a read primitive at the boundary, a
  resolver written in Skuld over UDP, and a TLS dependency, and each changes
  what the project is.
- Network tests stay hermetic. `cli/tests/network.rs` starts its own server on
  an ephemeral loopback port and stops it; nothing in the suite touches the
  network, and `tests/pass` has no server at all, so the library halves are
  tested there and the socket half only in `cli/tests`.
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

Function values are non-escaping and stack-allocated: a parameter or a local,
never a field, a return type, an array element or a payload. That rule is the
safety argument, not a detail — relaxing it reintroduces cycles the reference
counter cannot collect. Interfaces stay future work with unsettled receiver
syntax; storing a callback waits for them.

Do not implement generics, macros, async/await, threads, channels, reflection,
decorators, annotations, a package registry, compiler plugins, compile-time
execution, operator overloading, user-defined conversions, LLVM or Cranelift
before Demo 3. Interfaces and the official formatter remain future work
unless explicitly included in the active task. The FFI arrived with M5 and
authorizes nothing beyond itself: no standard library, no sockets, no wrapper
around a C library shipped with the compiler.
Do not build the standard library or a memory-management runtime ahead of need:
an addition to `std/` needs a caller in a milestone already planned.

The milestones proposed in `ROADMAP.md` do not relax any of the above.
HTTP and JSON are Planned and each needs its own
authorization; `extern "C"` and modules are implemented, and neither is a
standard library: a module system says where code lives, not what ships. `Result` arrived as a builtin, which settles that roadmap
question; general generics remain excluded and still need their own explicit
decision. M4's closing marker was met by `tests/pass/json_parser.skuld`, a
parser written in Skuld — it is a fixture, never a JSON facility in the
compiler, and nothing about it authorizes one. Modules and `import` landed by
explicit user decision, under the shape recorded in the status section above;
the reserved `std` prefix arrived with M7, whose library is small by design.
Nothing in that document authorizes extending it beyond an entry with a caller
in a planned milestone, nor a networking runtime or process execution from the
compiler library.

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
