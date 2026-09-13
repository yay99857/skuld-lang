# Skuld roadmap — Planned

This document records the **proposed** sequence for the milestones after the
current one. Nothing here is implemented, and recording a milestone here is not
authorization to start it: `AGENTS.md` remains the authority on which milestone
is active, and the user selects it explicitly. Read `LANGUAGE.md` for what the
language actually does today and `README.md` for what currently runs.

The long-range target that motivates this ordering is a program that performs
an HTTP request and decodes a JSON response. M1–M5 build the language
foundations it needs without reaching it; M6–M9 turn what remains into a library
problem and finally spend that budget. The naming below is local to this
document.

## M1 — Enums and `match` — Implemented

User-declared sum types with payloads, and exhaustive `match`.

```skuld
enum JsonValue {
    Null
    Bool(bool)
    Number(float)
    Text(string)
    Items([]JsonValue)
}

match value {
    JsonValue.Text(s): print(s)
    JsonValue.Number(n): print("${n}")
    _: print("other")
}
```

Implemented with declaration in a type namespace, unit and payload variant
construction, exhaustive match checking with arm payload bindings, and
managed reference-counting under leak and sanitizers. Direct value recursive
variants are rejected with `E0103`; indirect recursion via arrays or classes
is supported. Matches accept `:` and `->`, statements or blocks, and bindings
scope to each arm. Break and continue inside match arms naturally bind to
enclosing loops.

## M2 — `for` and iteration — Implemented

```skuld
for i in 0..len(bytes) { ... }
for item in items { ... }
```

Implemented with half-open integer ranges `a..b` and iteration over arrays
`[]T` by value. Range endpoints and collections evaluate once before iteration;
if `start >= end` or the collection is empty, the loop executes 0 times.
Loop variables are immutable bindings scoped to the body. Managed array elements
are retained and released cleanly on every iteration and exit path (`break`,
`continue`, `return`). `break` and `continue` naturally bind to the loop.

## M3 — `Result<T, E>` and propagation — Implemented

```skuld
func parse(text: string) -> Result<JsonValue, JsonError> {
    let n = parse_number(text)?
    ...
}
```

The error mechanism has to exist before anything that can fail exists.

- **Decision taken:** builtin `Result`, following the existing `Option`
  precedent. It avoids two-parameter generics and matches what was already
  there, at the cost of known debt against a future generic system. General
  generics stay a milestone of their own and `AGENTS.md` still defers them.
- **Implemented:** `Result<T, E>`, `Ok`/`Err`, `if let Ok(x) =`,
  `if let Err(e) =`, `match` with exhaustiveness over the two variants,
  `is_ok()`/`is_err()`, and `?` as an early return. Both constructors require an
  expected `Result` type, since neither payload can be inferred from the other,
  and no bare value wraps implicitly. `?` refuses a mismatched error type rather
  than converting it. `Ok` is tag 0 and `Err` tag 1, so a `Result` shares the
  enum layout, matching and reference-counting code.
- **Out of scope, and still out:** automatic error conversion across types,
  backtraces, recoverable panics.
- **Risk closed:** `?` emits the same cleanup attributes as a written `return`,
  so an early return releases the operand and every local acquired before it.
  `tests/pass/result_propagation.skuld` fails on the first `?` and on the second,
  the latter with an array, a class and a string alive, and runs under the
  address, leak and UB sanitizers with the rest of `tests/pass`.

## M4 — Bytes, sized integers and string slices — Implemented

Being able to look inside a string and build one from bytes.

- **Implemented:** the full set of sized integers `i8 i16 i32 i64` and
  `u8 u16 u32 u64`, taken at once because the machinery is shared; `[]u8`; byte
  indexing into a string; `[a..b]` slicing of strings and arrays;
  `bytes_to_string()` returning `Result<string, string>` after strict UTF-8
  validation; string building as a `[]u8` plus the existing growth operations.
- **Decision taken:** converting between widths is an explicit call on the
  target type's name (`u8(v)`, `int(v)`), trapping when the value does not fit.
  The roadmap had not settled this, and Skuld has no implicit coercions to fall
  back on. A literal instead takes the width its context expects and is
  range-checked where it is written.
- **Why after M3:** converting bytes to a string must fail on invalid UTF-8.
  Without `Result` the only option is a trap, which is wrong for network data.
- **Out of scope, and still out:** encodings other than UTF-8, a `char` or code
  point type, normalization, regular expressions. Integer/float conversion was
  not part of this milestone either.
- **Risk resolved:** slices copy. A three-byte view must not hold a large buffer
  alive, and a benchmark can revisit that later. The one exception is a slice of
  a string literal, whose bytes are static and outlive every slice of them, so
  no copy is needed and none is observable.
- **Still provisional:** the error side of `bytes_to_string` is a message rather
  than a dedicated error type. No error enum belongs in the language before a
  standard library exists to own one.
- **Closing marker — reached:** `tests/pass/json_parser.skuld` is a complete
  JSON parser written in **pure Skuld** over `[]u8`, returning
  `Result<JsonValue, JsonError>`. It scans bytes rather than characters and
  covers objects, arrays, strings with every escape including `\uXXXX` and
  surrogate pairs, numbers with fraction and exponent, the three keywords,
  a nesting limit and a canonical renderer, with the failure cases as part of
  the same fixture. That is the whole of `res.json()` except for where the
  bytes come from. The compiler gained nothing: the parser is ordinary user
  code, which is the point of the marker.
- **Found by the marker, not fixed by it:** the expected width of a conversion
  call reaches into its argument, so `u8(128 + n % 64)` is rejected where `n`
  is an `int` and only a named intermediate gets through. Range-checking a
  literal argument at compile time is what the context is for; propagating it
  through a whole expression tree is not. Left as it stands, since it is a
  checker change rather than a milestone one.

## M5 — `extern "C"` FFI and linking — Implemented

```skuld
unsafe extern "C" {
    func write(fd: i32, buffer: *u8, count: u64) -> i64
}
```

The boundary with the outside world, and the real gate for any network work.

- **Implemented:** `unsafe extern "C"` blocks of body-less declarations, the
  raw pointer type `*T` over scalars and `*void`, `ptr(value)` borrowing the
  bytes of a string or an array of scalars, foreign calls checked like any other
  direct call and emitted under the declared linker name, and `-l`/`-L`
  arguments forwarded from `build` and `run` to clang.
- **Decision taken:** the marker for the unsafe boundary is `unsafe` on the
  block, not on each call. The unsafe act is asserting a signature the compiler
  cannot verify against the library that is eventually linked; the calls that
  follow are then ordinary calls.
- **Decision taken:** libraries are named on the command line rather than in the
  source, and only `-l<library>` and `-L<directory>` are accepted, so a linker
  argument can never redirect clang's output or change how the program itself is
  compiled. Which library provides a symbol is a property of the build, not of
  the language.
- **Decision taken:** marshalling is explicit. There is no implicit conversion
  from `string` or `[]u8` to a pointer — Skuld has no implicit coercions
  anywhere else either — so `ptr(value)` borrows the bytes and the length
  travels separately, as `u64(value.len())`.
- **Open risk, closed as planned:** managed values never cross the boundary, and
  the checker enforces it rather than the programmer remembering it. A
  signature accepts only scalars, raw pointers and a `void` return; a `string`,
  array, class, `Option`, `Result` or a pointer to one is rejected (`E0103`).
  A borrow retains nothing and is valid only while its operand is alive, so a
  foreign call adds no retain, no release and no cleanup attribute of its own.
- **Validation:** `tests/pass/extern_c_ffi.skuld` calls libc `write` and `abs`,
  borrowing both a string and a `[]u8`, and routes every line of output through
  `write` so the fixture does not depend on stdio buffering. It runs under the
  address, leak and UB sanitizers with the rest of `tests/pass`, which is what
  proves the borrow leaks nothing. The `read_file()` target named earlier was
  dropped: opening a path makes a fixture depend on the filesystem, and
  `tests/pass` stays hermetic. `examples/ffi.skuld` is the same shape as a
  runnable example.
- **Out of scope, and still out:** Skuld callbacks into C, structs by value
  across the ABI, varargs, pointers to pointers, pointer arithmetic, reading or
  writing through a pointer from Skuld, and any ABI other than C. A declaration
  that disagrees with a header the generated program already includes is a clang
  error at build time, not a Skuld diagnostic.
- **Still provisional:** a Skuld string is not NUL-terminated, so calling a C
  function that expects a C string means building a `[]u8` with an explicit
  trailing `0`. Whether a helper for that belongs in the language or in the
  standard library is a question for M7, not for the boundary.

## M6 — Modules and `import` — Planned

Where a function lives, which is the question every milestone after M5 runs
into first. `import` is currently a reserved word the parser rejects.

```skuld
import "json"
import "net/socket"
```

- **In scope:** a compilation unit larger than one file, a path-to-file mapping
  rule, per-module name resolution tables, an export marker, and the CLI
  learning to compile a set of files rather than one.
- **Out of scope:** a package registry, versioned dependencies, remote imports,
  conditional compilation, and separate compilation with a cached artifact per
  module. The compiler keeps reading every source of a program.
- **Open risk:** the resolver's single-source declaration/use tables assume one
  `SourceFile`. Either they grow a module dimension or a module-level table sits
  above them; deciding that before writing code is the whole of this milestone's
  design work. Spans stay byte offsets into their own file.
- **Decisions taken:** a module is a **directory**; every `.skuld` file in it
  shares one namespace, as a Go package does. A name leaves it through an
  explicit **`pub`**, not through its spelling — tying visibility to a capital
  letter would fight the lowercase style the prelude already established.
  An imported name is **always qualified** by the module's last path segment
  (`json.parse`), so no import can quietly shadow a local name. An **import
  cycle is a diagnostic**, which also keeps initialization order defined.
- **Validation:** a two-module program in `tests/pass`, plus fail fixtures for
  a missing module, a cyclic import and a private name used from outside.

## M7 — A minimal standard library — Planned

The first code that ships with the compiler instead of inside it. It only
becomes possible once M6 says where it lives, and it is the natural owner of
the error types the language has so far been unable to name.

- **In scope:** a `std` written in Skuld, small on purpose — an error type for
  `bytes_to_string()` to return instead of a `string`, string helpers built on
  `[]u8`, and whatever M6 and M8 prove they need.
- **Out of scope:** collections beyond arrays (no map, no set — a hash map is
  its own milestone), formatting beyond interpolation, time, randomness,
  filesystem traversal, threads.
- **Decision taken:** `std` is a **reserved import prefix** resolved to sources
  embedded in the compiler binary, never to the filesystem. `import "std/utf8"`
  therefore works from any directory with no installation step, and a user
  directory named `std` in the program root neither shadows it nor is reachable
  as it; every other path keeps M6's rule of resolving under the program root.
  A compiler-supplied search path was rejected for creating a real installation
  step and making a program's compilability depend on its environment; the cost
  accepted in exchange is that updating the library means rebuilding the
  compiler, which is the right trade while the library is small and moves with
  the language.
- **Depends on:** M6 for module boundaries, M5 for anything touching the OS.
- **Open risk:** a standard library written before its users exist becomes a
  museum of guesses. Each entry needs a caller in a milestone already planned,
  or it stays out.
- **Marker:** `bytes_to_string()` returns `Result<string, Utf8Error>` and the
  provisional string error side of M4 disappears.

## M8 — Function values, callbacks and interfaces — Planned

```skuld
numbers.sort((a, b) => a - b)
```

The abstraction milestone the earlier ones kept deferring. Sorting has demanded
it since M2 and the sketch in `test.skuld` is marked unsatisfactory by the user,
so the syntax is an open question, not a decision this document may take.

- **In scope:** function types as values, a lambda form, passing them as
  arguments, `sort()` over arrays as the first consumer, and interfaces with
  `impl` as the named-abstraction half.
- **Out of scope:** closures capturing mutable state, generic functions,
  higher-kinded anything, dynamic dispatch beyond what interfaces need.
- **Open risk:** capture and the reference-counted model. A lambda that
  captures a managed value must retain it, which makes the lambda itself a
  managed value, and capturing `this` inside a class creates exactly the cycle
  the project has no collector for. The cheapest answer — non-capturing
  function values only, enough for `sort()` — is on the table and would keep
  this milestone small.
- **Open question:** whether interfaces belong here at all or in a milestone of
  their own. `LANGUAGE.md` lists them as Planned with unsettled receiver syntax.

## M9 — Sockets, HTTP and the long-range target — Planned

```skuld
let res = http.get("http://example.com/data.json")
let value = json.parse(res.body)?
```

The target that motivated the whole ordering, spent at last, and by then not a
language milestone: an FFI binding over libc sockets, a response parser in
Skuld, and the JSON parser that closes M4.

- **In scope:** a socket binding over M5, a blocking HTTP/1.1 client, and the
  JSON value and parser from M4's closing marker promoted into `std`.
- **Out of scope, and pointedly:** TLS, so plain HTTP only; a server; async,
  non-blocking I/O and any event loop; connection pooling; HTTP/2.
- **Open risk:** TLS is where this stops. Binding a system TLS library is a
  milestone of its own and arguably a dependency policy decision, not a
  technical one.
- **Validation:** an integration test against a local socket that this
  repository starts, never the network. `tests/pass` stays hermetic.

## A parallel track — tooling — Planned

Not sequenced with the milestones above and not blocking any of them.
`LANGUAGE.md` names `fmt`, `test`, `doc` and `new` as future commands; the
official formatter in particular has been deferred since the beginning and can
land whenever the syntax stops moving.

## Beyond M9

Nothing here is planned in the sense the sections above are. The recurring
candidates are a real map type, generics (question 1 below), LLVM or Cranelift
as an alternative backend, portability beyond the current target, and eventual
self-hosting. Each would need its own design pass and its own explicit
authorization.

## Open design questions

These are not settled by this document and change the shape of the milestones
above.

1. ~~Builtin `Result` or general generics?~~ Answered: M3 shipped `Result` as a
   builtin. Whether general generics ever enter the project, and how they would
   reconcile with the builtin `Option` and `Result`, remains open.
2. ~~Do string slices retain their owner or copy?~~ Answered: they copy, except
   for slices of string literals, whose bytes are static. Whether a retaining
   slice earns its danger is a question for a benchmark, not for this document.
3. Recursive enum variants: automatic boxing, or the user's responsibility?
4. Callback and lambda syntax, deferred by M2 but eventually demanded by
   sorting, and now the subject of M8. The current sketch in `test.skuld` is
   marked unsatisfactory by the user.
5. ~~JSON objects as a list of key/value fields, or waiting for a real map
   type?~~ Answered by M4's closing marker: a list of key/value pairs in source
   order, with linear lookup and duplicate keys preserved. Waiting for a map
   would have made the milestone unclosable, since a hash map is a milestone of
   its own and is not authorized. Whether the parser M9 reuses keeps that
   representation is open again once a map exists.
6. ~~Where module boundaries sit — file or directory, `pub` or convention?~~
   Answered for M6 by the user: a module is a **directory**, export is an
   explicit **`pub`**, use is always **qualified** (`json.parse`), and an import
   cycle is an error. Whether the resolver grows a module dimension or gains a
   table above it is an implementation question left to that milestone. Where a
   module path is **rooted** was left open by M6 and answered separately by the
   user for M7: a reserved `std` prefix over sources embedded in the compiler
   binary, with every other path still relative to the program root.
7. Whether function values may capture. Non-capturing values are enough for
   `sort()` and create no cycles; capturing ones make a lambda a managed value
   and can capture `this`, which the reference counter cannot collect.
8. Whether interfaces are part of the callback milestone or a milestone of
   their own.
9. Where NUL-terminated C strings are built — a language helper, or a standard
   library function over `[]u8`. M5 left it to the caller.
