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
  `User { ... }` syntax. Field defaults arrived with M14, so `new User()` is
  written where every field has one; a user-defined constructor with a body
  still needs design and is deliberately absent;
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
  Managed payloads retain/release only when present. Interfaces arrived with
  M10; general generics remain future work.
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
  released. Sorting and callbacks arrived with M8: `sort()` is stable, returns
  void and takes a non-escaping comparator.
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
  M10 (interfaces) is complete, delegated the same way: conformance is declared
  on the class (`class User: Printable`) and checked method by method, only
  classes implement one — a struct would have to be boxed — and an interface
  value is storable, which is what M8's escape rule left out. An interface
  value is the object plus a method table; the allocation header already
  carries each class's destructor, so counting one needs no new runtime code.
  Inheritance between interfaces, default bodies, structs behind interfaces and
  run-time downcasts are all out and need their own decision.
  M9 (sockets, HTTP and the long-range target) is complete: `std/net` is a
  blocking TCP connection over libc, `std/http` an HTTP/1.1 client on top of
  it, and `std/json` is M4's parser promoted out of its fixture. A Skuld
  program fetches a document and decodes it.
  M11 (the official formatter) is complete: `skuld fmt <file>` formats in-place
  and `skuld fmt --check <file>` detects drift without writing. It preserves all
  `//` comments, exact literal representations, statement boundaries and AST
  semantics. Return types normalize to `-> Type` while `:` remains fully accepted
  in source. All fixtures and examples format idempotently and execute cleanly.
  M12 (references and rename in the LSP) is complete; its two open decisions
  were taken as recorded below and in `ROADMAP.md`.
  M13 (local CLI applications and `skuld test`) is complete, with its file,
  process and test-discovery decisions recorded below and in `ROADMAP.md`.
  M14 (field defaults) is complete; a constructor with a body is not part of
  it, for the reason recorded below. M15 (a string-keyed map), M16 (host names
  and error detail), M17 (verified HTTPS) and M18 (measured performance and a
  second native target) are complete, each with its decision recorded below and
  in `ROADMAP.md`. M19 (bits and integer literals) is complete: `&`, `|`, `^`,
  `~`, `<<`, `>>` and compound forms `&=`, `|=`, `^=`, `<<=`, `>>=`; `0x`, `0b`,
  `0o` literals; `_` as a digit separator in all bases; bitwise operators bind
  tighter than comparisons; shifts trap if amount is negative or `>= width`;
  `>>` on signed types is arithmetic and on unsigned types is logical; `>>`
  coexists with generic type closures via a parser splitting rule, answering open
  question 9; expression lambdas `(a, b) => expr` and array `to_sorted` were
  also completed and tested. M20 (named constants and value patterns) is complete:
  `const NAME: Type = expr` and `pub const` at module and local function scope,
  compile-time evaluation for literals, arithmetic, bitwise, string concatenation,
  and conversions, lowered inline with zero runtime registers and stack slots;
  `match` extended to scalar and string targets with literal/constant patterns,
  range patterns (`a..b`, `a..=b`), and mandatory wildcard `_` for exhaustiveness;
  `std/net`, `std/dns`, `std/fs` and `std/os` name every protocol number, flag and
  `errno` value.
  M21 (word-sized integers and conversions) is complete: `usize` and `isize` as
  distinct word-sized integer types matching target pointer width (`size_t`/`ptrdiff_t`),
  explicit trapping float-to-int conversions (`int(f)`, `i8(f)`..`u64(f)`, `isize(f)`,
  `usize(f)`) truncating towards zero and trapping on out-of-range values and NaN;
  explicit int-to-float conversions `float(i)`; distinct `char` scalar type with `'a'`
  literals, comparisons, string interpolation `${c}`, `print(c)`, explicit conversions to/from
  `u8`/`u32`/integers trapping on invalid Unicode code points, and pattern matching on char literals
  and ranges. `extern "C"` declarations use `usize`/`isize` for `size_t`/`ssize_t`, and
  `portability.rs` runs 100% of pass fixtures on i686 with no exceptions left (`TARGET_SPECIFIC = &[]`).
  M22 (fixed-size arrays and stack buffers) is complete: the type `[N]T` with
  value semantics, `N` a constant expression greater than zero, the repeated
  literal `[element; count]` and list literals inferred against an expected
  `[N]T`, indexing bounds-checked at compile time for a constant index and at
  run time otherwise, `len()` as a compile-time constant, `arr[a..b]` copying
  into an owned `[]T`, and `ptr(arr)` borrowing `*T` for a scalar element type.
  A fixed array lives inline: on the stack, inside a struct, and — the
  milestone's open question, decided yes — inline in a class allocation, with
  the same rules the object's other fields have, which is sound because a fixed
  array is not itself reference counted. `[N]T` widens into `[]T` without
  allocating, as an immortal stack view; a view is read-only, and a mutating
  call on one traps through `*capacity == 0` in `skuld_array_reserve`, so a
  stack buffer is never reallocated onto the heap behind a reference. Escape is
  a diagnostic rather than a trap wherever the compiler can see it: returning a
  local fixed array as a `[]T`, or storing one into a heap array field, is
  rejected. The closing marker is met — `std/fs`'s `read_file` and `std/net`'s
  `receive_all` and `open` read into stack buffers instead of pushing byte by
  byte, and `tests/bench/strings.skuld` formats integers through one.
  M23 (pointers that can be read) is complete. `unsafe { ... }` is a statement
  block, and it is where the guarantees the compiler makes everywhere else are
  suspended *and say so*: everything outside one keeps every rule it had, and
  the block itself changes nothing about the generated code. Inside one,
  `load(p)` and `store(p, v)` read and write through a `*T`, `volatile_load`
  and `volatile_store` do the same where the backend may neither elide nor
  reorder the access, `offset(p, n)` steps in element units, `addr(p)` and
  `ptr_from(a)` convert between a pointer and a `usize`, and `ptr(local)` takes
  the address of a scalar local. They are prelude bindings like `print` and
  `u8()`, shadowable like them. A load takes its type from the pointer's own
  pointee, so nothing spells it at the call: there is no `load<u32>(p)` and no
  `*p` operator, and `ptr_from` is the one that reads the expected type from
  its context, as `None` already does. A pointer still points only at a scalar
  or `void`, so no pointer read ever produces a managed value the counter never
  saw allocated; `*void` can be carried and converted but not read, and a
  pointer to a pointer is still not a type — an address travels as a `usize`.
  Two rules carry the safety argument and must survive: `unsafe` is lexical, so
  a function called from inside a block is not inside it and needs its own,
  which keeps the unsafe surface countable; and `ptr` over a local requires a
  `var`, since a pointer can always write and a `let` is what the backend reads
  to borrow instead of counting. The foreign boundary did not widen: a
  signature still refuses managed types and `ptr()` still borrows. The closing
  marker is `tests/pass/pointers.skuld`, a bump allocator and an intrusive
  linked list over `malloc`ed memory, clean under the address, leak and UB
  sanitizers.
  Two additions followed M24, both from trying to write a real status-bar
  module in Skuld and finding what stopped it. Any pointer now converts to
  `*void` where one is expected: it is the one pointer conversion that cannot
  be wrong, since `*void` claims nothing about a pointee and nothing can be
  read through it, and without it every call taking a `void *` was written
  `ptr_from(addr(p))` inside an `unsafe` block — a block that suspends real
  guarantees, to express a conversion that suspends none. And `std/os` gained
  `run(program, arguments)` and `environment(name)`: the caller is that module,
  which is what an addition to `std/` needs. `run` looks the program up on
  `PATH`, waits, and answers the exit status, the output and whether it was
  truncated at 64 KiB; there is **no shell**, so nothing is expanded, split or
  quoted, a missing program answers 127 and a signalled one -1, and standard
  error is left alone. Its `argv` is the pointer-to-pointer the boundary
  refuses, so it is built in raw memory with `store`, and `size_of` over a
  one-pointer `extern struct` is how the module asks how wide a pointer is
  here. `environment` reads `/proc/self/environ` rather than calling `getenv`,
  whose prototype takes a `char *`. That is the general rule worth remembering:
  the generated program includes the C standard headers, so a function declared
  there with a `char *` or a `FILE *` cannot be declared in Skuld at all, while
  the POSIX functions those headers leave out are free to declare — and two
  modules may declare one C function only if they declare it identically, as
  `std/fs` and `std/os` both declare `read`.
  M24 (layout and the ABI) is complete. The ordinary `struct` layout stays the
  compiler's own and deliberately unspecified; a type that describes memory
  somebody else defined is written `extern struct Name { ... }` and is laid out
  the way the platform's C compiler lays out the same fields, with `packed` and
  `align N` between the name and the body. Both are read as ordinary
  identifiers in that one position — the decision here was that no attribute
  syntax enters the language, since `annotations` are excluded by design and a
  general one would then have to be generalised. Members are restricted to what
  C can describe (the scalars, a raw pointer, a fixed array of those, another
  `extern struct` or `extern union`), so a managed value can never sit inside a
  layout C decides. Such a type crosses the boundary by value in both
  directions, and `ptr()` takes its address as `*void`. `size_of(Type)` and
  `offset_of(Type, field)` answer in bytes as a `usize`, refuse a type the
  compiler laid out, and are answered by the C compiler for the target being
  built rather than recomputed here — so neither is a compile-time constant and
  neither can initialize a `const`. `size_of` was added alongside `offset_of`
  because a call that takes a pointer to a structure takes its length beside
  it. `extern union Name { ... }` is one piece of memory read as one of several
  types: it is constructed one member at a time, writing a member is what makes
  it live and needs no claim, and **reading** one is written inside `unsafe`,
  since which member is live is the program's claim. An enum may name the
  integer type its variants are worth — `enum Protocol: u8 { Tcp = 6 }` — where
  a variant without a value continues from the one before it, two variants
  worth the same number are refused, a payload excludes numbering, `u8(p)`
  converts to the number and `Protocol(v)` converts back, trapping on a number
  no variant is worth. A numbered enum is not itself a foreign type; it crosses
  as the integer it converts to. The closing marker is met: `std/net`'s
  `SockaddrIn` and `std/dns`'s `Timeval` are declared types measured with
  `size_of`, `htons`/`htonl` replaced the arithmetic that assumed a
  little-endian machine, and `cli/tests/portability.rs` runs every fixture on
  i686, where `timeval` really is eight bytes. `cli/tests/abi.rs` compiles a C
  object and links it, which is the only way to observe a struct passed by
  value actually arriving.
  M25 (`defer` and deterministic cleanup) is complete. `defer statement` runs
  the statement when the block it was written in is left, in reverse order of
  registration, on every way out: falling off the end, `return`, `break`,
  `continue`, a `?` that propagates and a `let ... else` escape. It does not
  run on a trap, since a trap ends the process and there is nothing to unwind.
  Three decisions are taken and should not be reopened by accident. There is
  **no `errdefer`** — one kind of deferred statement is easier to reason about
  than two, and where a resource is released on failure but handed over on
  success a flag the `defer` reads says which happened, which is what
  `std/tls` now does. A deferred statement is **run at the exit, not recorded
  at the `defer`**: it reads what its names are worth when the block ends,
  which is what the line looks like it does (Go records the arguments instead;
  this follows Zig). And **nothing leaves a deferred statement** — `return`,
  `break`, `continue` and `?` are refused inside one, as is a declaration,
  which would bind a name where nothing can read it. The value a `return`
  hands over is settled before the deferred statements run, so a `defer` never
  changes what the caller receives. The backend emits the statements again at
  each exit rather than jumping to them, which is what lets them read the
  locals they were written next to; a loop body is the frame `break` and
  `continue` unwind to. The closing marker is met: `std/tls`'s `connect`
  released a socket, a context and a connection on each of six failure paths
  and now releases each once, `std/net`'s `open` does the same, and both
  suites — including the expired, wrong-name and untrusted certificate cases —
  stay green and sanitizer-clean.
  M26 (freestanding Skuld) is under way. Its first piece is `static`: module
  level storage, written like a constant and behaving like a variable, whose
  initialiser is evaluated at compile time because there is no moment before a
  program starts at which one could run. A static holds a scalar or a fixed
  array of scalars starting at zero; a managed value is refused, since nothing
  would retain what outlives every call and nothing would release it. An array
  starts at zero because C has no repeated initialiser and a written-out one is
  not an initialiser a reader wants. It is available in both build modes — a
  hosted program has the same need — and `tests/fail/reserved_declaration` was
  promoted to `tests/pass/statics`, which emptied that tripwire.
  The rest of M26 is complete too, which closes the systems sequence.
  `--freestanding` on `check`, `emit-c` and `build` compiles a program with no
  runtime and no libc: the generated C carries neither `runtime/strings.c` nor
  a generated `main`, includes only the four headers C guarantees a
  freestanding implementation, and a trap becomes `__builtin_trap()` because
  there is no standard error to explain itself to. Bounds checks and overflow
  traps stay — `skuld_index` moved out of the managed runtime and into a
  prelude both modes share, since checking an index is part of the language and
  not part of managing memory. It is **one language, two build modes**: the
  checker refuses the values that would need a runtime — `string`, `[]T`, a
  class, an interface, a `weak`, anything holding one, and `print` — and
  changes nothing else. A `pub func` is emitted under the name it was written
  with, so an assembly stub or a bootloader has something to call; everything
  private keeps a generated name; `main` is neither generated nor looked for. A
  freestanding build writes an **object file** and accepts no linker argument:
  the linker script, the target and the startup stub belong to whoever
  assembles the result, and for a target that is not this machine `emit-c
  --freestanding` hands over the C the way the 32-bit portability suite already
  does. Decided here and not to be reopened: a freestanding program is **given
  no allocator, because nothing allocates** — its storage is its statics and
  its stack, and one that wants a heap manages a region itself with M23's
  pointers; an interface would not have helped, since an interface is a class
  and a class is what this mode does not have. The closing marker is met in
  `cli/tests/freestanding.rs`: a static Linux x86-64 executable making `write`
  and `exit` syscalls through an assembly stub, checked to be statically linked
  rather than assumed to be, and a 32-bit multiboot kernel that writes to the
  VGA text buffer, reads it back and stops the machine — built and checked for
  its multiboot header everywhere, booted under `qemu-system-i386` where that
  exists. What a freestanding program cannot do without help is exactly what
  the language deliberately cannot spell — `syscall`, `in`/`out`, anything that
  is one instruction rather than a value — and those are assembly declared
  through `extern "C"`, which is the boundary the FFI already was.
  No milestone is active. The candidates `ROADMAP.md` lists after the systems
  sequence — inline assembly, atomics and a memory model, threads, interrupt
  and naked calling conventions, linker sections, a target that is not Linux —
  are each a milestone of its own and none is authorized. One older decision
  should not be reopened by accident either: M20's `const` does not replace
  `let`, since an immutable binding is what the backend reads to borrow a
  managed local instead of counting it.
  Do not infer authorization for further features without explicit decision.
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
  offsets) and `std/cstring` (`to_c`, refusing an embedded NUL); M9 added
  `std/json`, `std/net` and `std/http`; M13 `std/fs` (whole files by path),
  `std/os` (arguments, flush, exit, and now `errno`) and `std/testing` (the
  assertions `skuld test` runs); M15 `std/map`; M16 `std/dns`; and M17
  `std/ffi`, `std/tls` and `std/https`, the last two being the only modules
  that need a library named on the command line. It stays small
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
- Interfaces are implemented and only classes implement them. Conformance is
  declared, never inferred from a class that happens to have the methods, and
  each is checked with parameters and result matching exactly. Dispatch is
  dynamic through per-class thunks, so no call goes through a mismatched
  function pointer. A class widens into an interface it declared like a value
  wraps into an expected Option, and the two compose in that order — the only
  place two implicit conversions meet.
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
- The three limits the network library used to have are closed, each by the
  decision recorded in `ROADMAP.md`, and none of them by a read primitive at
  the foreign boundary. Name resolution is `std/dns`, a DNS client written in
  Skuld over a `connect`ed UDP socket: IPv4 A records, servers from
  `/etc/resolv.conf`, a three-second timeout, no cache, and a query id from the
  clock, since the language has no random source. Error detail is a runtime
  bridge like M13's — `sk_errno` and `sk_error_message` — so `std/fs` and
  `std/net` carry an `OsFailure` of what was attempted and the system's number.
  TLS is OpenSSL through the FFI, quarantined in `std/tls` and `std/https`:
  `std/http` still refuses `https://` and links nothing, and a program that
  wants TLS imports it and passes `-lssl -lcrypto` itself. Verification cannot
  be turned off from Skuld and there is no `insecure` flag. `std/ffi` adds the
  only two pointer operations that read no memory — `null()` and `is_null()` —
  because a library that allocates answers with NULL. Do not add a
  dereferencing primitive; that rule is what makes this FFI safe.
- `std/map` is a `StringMap`: string keys, integer values, open addressing,
  iteration in insertion order. It is an index — the value is a position, a
  count or an identifier, and what is indexed stays in the array that holds it,
  which is why JSON keeps its member order and duplicates. It is not a general
  collection, and choosing it authorized neither generics nor a builtin map.
- Network tests stay hermetic. `cli/tests/network.rs` starts its own server on
  an ephemeral loopback port and stops it; nothing in the suite touches the
  network, and `tests/pass` has no server at all, so the library halves are
  tested there and the socket half only in `cli/tests`.
- The official formatter is implemented: `skuld fmt <file>` formats in place and
  `skuld fmt --check <file>` reports drift without modifying the file. Formatting
  preserves all line comments (`//`), literal byte choices, statement boundaries
  and AST semantics. Top-level and method signatures normalize to `-> Type`, while
  colon return type syntax remains accepted. Match arms normalize to `Pattern: ...`.
  Idempotency and native output preservation are tested across all fixtures.
- The language server finds references and renames. A workspace is the set of
  programs the editor has open: each document is compiled as the entry file of
  its own program, so a module's uses are found through whichever entry file
  reaches it, a program nothing open reaches is not searched, and closing a
  document forgets its check. Renameable names are functions, parameters and
  locals — exactly what the resolver's tables record. A prelude binding, an
  import qualifier, a type name, a field, a method and anything from the
  embedded standard library are refused with their reason; type names would
  need the checker to record where a type is written, which is a compiler
  change. A rename is verified rather than trusted: every program the edit
  touches is checked again over the edited texts and the map from each name to
  the declaration it reaches is compared before and after, so a capture that
  would still compile is refused. A buffer that has changed since it last
  checked is refused, since its recorded offsets describe a text that is gone.
  The server writes nothing: a workspace edit is the client's to apply.
- The server also answers `textDocument/documentHighlight`,
  `textDocument/signatureHelp`, `textDocument/inlayHint`,
  `textDocument/semanticTokens/full` and `/range`, `textDocument/documentSymbol`,
  `textDocument/formatting`, `textDocument/hover`, `textDocument/completion`,
  `textDocument/definition`, `textDocument/declaration`,
  `textDocument/typeDefinition`, `textDocument/implementation`,
  `textDocument/foldingRange`, `textDocument/selectionRange`,
  `textDocument/prepareCallHierarchy` in both directions,
  `textDocument/codeAction`, `workspace/symbol` and
  `workspace/didChangeWatchedFiles`. The outline is drawn from the syntax
  alone, so it needs no check at all, and it falls back to the last text that parsed rather
  than emptying itself mid-declaration; its entries are sorted by span, since
  the AST keeps each kind of declaration in a list of its own and a client does
  not re-sort what it is given. Formatting is the official formatter, whole
  document only — the formatter reads a program, not a fragment — and a file
  that does not parse or is already formatted is answered with no edits rather
  than an error, because the caller is usually format-on-save.
- A diagnostic may carry a `Fix`: the exact span to replace and the bytes to
  put there, in the diagnostic's own file. `help` stays prose for a person and
  a `Fix` is the same knowledge for a tool, so it exists only where the
  compiler knows the whole edit and applying it is never a guess: a missing
  `unsafe` on an `extern` block, a name one slip from one in scope, a
  misspelt field, method or enum variant, the arm a `match` does not cover,
  and a `let` that is assigned to. A suggestion is searched among exactly the
  names the use could have reached — the same chain of scopes `lookup` walks,
  a module's public names only, the members the receiver actually has — so it
  never fails for a second reason; over two mistakes nothing is suggested,
  since every short name reaches every other. A `match` arm needs the source
  text rather than the tree, to land at the indentation the block uses, which
  is why the checker holds the files. A missing field is deliberately offered
  no fix: which value goes there is the one thing the compiler does not know,
  and Skuld has no zero value to stand in for it. The server
  turns these into `quickfix` code actions and reinterprets nothing: it keeps
  what the last check offered, replaces those offers on every check, and drops
  an edit whose span no longer fits the buffer. The other code action is a
  `refactor.rewrite` that writes out an inferred binding type.
- A highlight is `references` narrowed to the document and widened in what it
  accepts: a prelude binding and an import qualifier are highlighted although
  neither can be renamed. It carries no `kind`, since the resolver records
  where a name is written and not whether that writing reads or assigns.
- Signature help finds the call over the text rather than the syntax tree,
  because a call is asked about exactly while it is half-written: the innermost
  unclosed `(` before the cursor and the top-level commas since it, skipping
  strings, characters and comments, and treating a `[` or `{` still open as
  having left the argument list. Parameter names come from the declaration as
  written — the checker's signature keeps types alone — and a prelude binding
  or a builtin method, neither of which was declared in a file, has its
  parameters recovered from the one line that describes it. A construction
  lists the fields and marks none active, since `new User(...)` names them.
- An inlay hint is the inferred type of a binding that writes none. Whether a
  type is written is decided by reading the text after the name, which is sound
  because a `:` follows a binding only in `let name: Type`; taking the names
  from the resolver's table rather than a walk is what makes a binding inside a
  lambda ordinary. A hint whose type is `Void` or `Error` is not drawn.
- Semantic tokens classify names only. Keywords, literals and comments stay
  with the editor's own highlighting, which a client layers underneath, and a
  name the check cannot place is left out rather than guessed at. The stream is
  the whole document each time: a delta would mean keeping the previous stream
  per document to diff against, which is state to go stale.
- Files, the process and tests arrived with M13. `std/fs` reads and writes a
  whole file **by path**: a handle would need a lifetime rule and Skuld has no
  destructor a user can write, so there is no `open`, no close and no streaming
  yet. `std/os` reaches the process through the runtime, not through libc
  directly: following `argv` is a pointer-to-pointer read the foreign boundary
  refuses, so `sk_arg_count`, `sk_arg_len`, `sk_arg_copy`, `sk_flush` and
  `sk_exit` are the whole bridge — scalars and a pointer to bytes the caller
  owns, nothing managed, nothing handed back. The generated `main` takes
  `argc`/`argv` and gives them to the runtime; that is the only place a program
  sees them. `main` still returns void, and a status comes from `os.exit(code)`,
  which flushes and releases nothing because the process is ending. A test is a
  top-level `func test_...()` taking nothing and returning nothing — a naming
  rule, not an annotation — and `skuld test` compiles the file once with an
  entry point it writes, runs the tests in source order in one process, and
  stops at the first failure, since Skuld has no recoverable panic. Each
  finished test prints a flushed marker so the record survives a trap. `skuld
  run file --args ...` hands everything after `--args` to the program, unread
  and `--` included; `--` keeps its own meaning of "every later argument is a
  path", and only `run` accepts `--args`, since nothing else executes a
  program.
- A field may carry a default: `name: Type = expression`, on a class or a
  struct alike. A field with one may be left out of `new C(...)` or `S { ... }`;
  a field without one must still be supplied, so an object is never partially
  initialized. There is no constructor with a body, and that is the reason
  defaults exist in this shape: a constructor would expose a `this` whose
  fields are not all set yet, which construction has always guaranteed against.
  Initialization that can fail is therefore a function returning a `Result`,
  not a failing constructor. A default resolves in the scope of the file that
  declares the type — `this` and sibling fields are not in scope, since there
  is no object when it runs — is checked once against the field's type where it
  is written, and is evaluated at every construction, in the declaring file's
  coordinates. Order is the usual one: written arguments first, left to right,
  then the defaults of the fields nobody wrote, in declaration order.
- Performance is measured, not asserted. `tests/bench/` holds the programs and
  `run.sh`, `BENCHMARKS.md` the report: between 1.3x and 2.2x the faster of
  Rust and Go across five workloads, faster than Go on two — the spread moved
  once already, when measuring found that every read of a managed local was
  taking a counted copy of it. A benchmark is a
  comparison only if the Skuld, Rust and Go versions print the same checksum,
  which is why each prints one before it is timed. An optimisation needs
  before-and-after evidence from these benchmarks and unchanged semantics, and
  the numbers in `BENCHMARKS.md` are updated with it; no speed ratio is
  promised anywhere.
- The second native target is i686-linux-gnu, and `cli/tests/portability.rs`
  runs every `tests/pass` fixture on it: 58 of 59 produce identical output with
  no change to the runtime. The exception is the foreign boundary, which is the
  only place a target leaks into the language — an `extern "C"` declaration
  names a concrete width and `size_t` is not one. Skuld has no `usize`; adding
  one is a language decision, not a portability fix. `std/net` and `std/dns`
  also assume a little-endian Linux and its numeric constants, and `std/dns`
  builds a `struct timeval` of two 64-bit fields, which is LP64-specific.
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
before Demo 3. Interfaces arrived with M10 and the official formatter with
M11; neither authorizes anything beyond itself. The FFI arrived with M5 and
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
