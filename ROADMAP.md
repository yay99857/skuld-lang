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
- **Found by the marker, fixed after it:** the expected width of a conversion
  call reached into its whole argument, so `u8(128 + n % 64)` was rejected where
  `n` is an `int` and only a named intermediate got through. Range-checking a
  literal argument at compile time is what the context is for; propagating it
  through an expression tree is not. The width now reaches a literal and stops
  there (`a062a8a`), and the fixture's `encode_utf8` no longer needs the
  intermediates the limitation forced on it.

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

## M6 — Modules and `import` — Implemented

Where a function lives, which is the question every milestone after M5 ran
into first.

```skuld
import "json"
import "net/socket"
```

- **Implemented:** a compilation unit larger than one file; `import "path"`
  before any declaration, resolved against the program root; `pub` as the export
  marker; per-module type and value namespaces; qualified names in every
  position a type or constructor can appear; and a CLI that compiles a set of
  files. Reading them stays with the caller: the compiler asks a loader, so the
  library still performs no I/O.
- **Not implemented here:** the reserved `std` prefix M7 records below. Every
  path this milestone resolves is a directory under the program root.
- **Out of scope:** a package registry, versioned dependencies, remote imports,
  conditional compilation, and separate compilation with a cached artifact per
  module. The compiler keeps reading every source of a program.
- **Risk resolved:** the resolver's declaration and use tables grew a file
  dimension rather than gaining a table above them. Spans stay byte offsets into
  their own file, so the file has to be part of the key; pretending a span
  identified a position on its own was the only alternative and it was wrong.
  Scopes nest prelude → module → file → bodies, which is what makes a module's
  declarations shared between its files while its imports stay per-file.
- **Decisions taken:** a module is a **directory**; every `.skuld` file in it
  shares one namespace, as a Go package does. A name leaves it through an
  explicit **`pub`**, not through its spelling — tying visibility to a capital
  letter would fight the lowercase style the prelude already established.
  An imported name is **always qualified** by the module's last path segment
  (`json.parse`), so no import can quietly shadow a local name. An **import
  cycle is a diagnostic**, which also keeps initialization order defined.
- **Also decided while implementing:** path segments are plain names, so a path
  cannot climb out of the program root; a method is public with the type that
  owns it; an `extern` block cannot be exported, since a foreign signature is an
  assertion by the module that wrote it; and the entry file is the root module
  on its own, so a directory of unrelated programs does not become one module.
- **Validated:** `tests/pass/modules` is a three-module program — a module split
  across two files, a class field typed from another module, qualified calls,
  construction and enum patterns, and an import shadowed by a local — running
  under the sanitizers. Eight `tests/fail` fixtures pin the cycle, the missing
  module, the private value, the private type, the undeclared name, the
  unqualified use, the misplaced `import` and the escaping path.

## M7 — A minimal standard library — Implemented

The first code that ships with the compiler instead of inside it, and the owner
of the error types the language had no way to name.

- **Implemented:** `std` as a reserved import prefix over modules written in
  Skuld and embedded in the compiler binary; `std/utf8` with a real `Utf8Error`
  and strict validation; `std/strings` with byte-offset helpers; `std/cstring`
  building the NUL-terminated buffer C expects.
- **Decision taken, by the user:** the library is rooted at a reserved prefix
  rather than a search path. `import "std/utf8"` never reaches the caller's
  loader, so a program compiles the same from any directory with no
  installation step, and a `std` directory beside a program is unreachable —
  a language rule, which is why the reservation lives in the compiler and not
  in the CLI's loader. A reserved path naming no module is an error listing the
  ones that exist, never a fallback to disk. The accepted cost is that changing
  the library means rebuilding the compiler.
- **Decision taken:** the builtin `bytes_to_string` was left alone. Making it
  return `Result<string, Utf8Error>` would have the checker depend on a name the
  library chose, inverting the dependency for the sake of a tidier marker.
  `std/utf8.decode` is the call with a real error beside it, and M4's
  provisional message stays where it is, now with something better next to it.
- **Open risk, held to:** a standard library written before its users exist
  becomes a museum of guesses. Every entry here has a caller in a milestone
  already planned — a response parser needs `trim`, `split` and `index_of`; the
  FFI needs `to_c`; bytes from a socket need `decode` — and nothing was added
  because it looked useful.
- **Validation:** `tests/pass/std_library.skuld` exercises all three modules
  under the address, leak and UB sanitizers; `tests/fail/std_shadow` proves a
  user `std` directory cannot replace the embedded one, and
  `tests/fail/std_unknown_module` that a reserved path never falls back to
  disk. A compiler unit test compiles every embedded module on its own, so a
  library that ships with the compiler cannot ship broken.
- **Out of scope, and still out:** collections beyond arrays, a map, formatting
  beyond interpolation, time, randomness, filesystem traversal, threads,
  networking, and any way to add to the library except by changing the compiler.

## M8 — Function values and lambdas — Implemented

```skuld
numbers.sort((a: int, b: int): int { return a - b })
```

The abstraction milestone the earlier ones kept deferring. Sorting has demanded
it since M2. The user delegated this milestone's three open questions; the
answers are below, and each is a consequence of a constraint the project
already had rather than a preference.

- **Decision — syntax.** A lambda is `(params): Type { body }` — the shape a
  method already declares itself with, so it needs no keyword. The user chose
  this over the `func(...)` form first proposed here, and it is the better fit:
  classes already declare `hello() -> string` without `func`. The function
  *type* keeps `->`, as `(int, int) -> int`, so that a parameter is not written
  `compare: (int, int): int` with `:` meaning both "has type" and "returns".
  Parameter and return types may be omitted when the expected type is known,
  which is the local inference `let` already does. The sketch's `=>` is not
  adopted.
- **Decision — capture.** A lambda may capture, by value, and a function value
  **may not escape**: it can be a parameter or a local, never a field, a return
  type, an array element or a payload of `Option`, `Result` or an enum. That is
  not a restriction chosen for comfort. Non-atomic reference counting with no
  collector means any managed object able to reach a closure that captured it is
  an uncollectable cycle; forbidding the closure from being stored anywhere
  managed removes the reachability instead of asking the user to reason about
  weak captures, which is the ownership burden `AGENTS.md` rules out. Because a
  function value cannot outlive the call it is passed to, it is stack-allocated
  and needs no retain, no release and no allocation at all.
- **Decision — interfaces are not part of this.** They are a different
  mechanism — named abstraction and dispatch, with receiver syntax `LANGUAGE.md`
  still lists as unsettled — and bundling them would double a milestone whose
  point is to stay small. They get their own milestone, after this one. Much of
  what interfaces are reached for in practice is callbacks, which this milestone
  covers, so the urgency drops rather than rises.
- **In scope:** function types, the lambda form, passing them as arguments,
  calling a function value, the escape rule as a diagnostic, and `sort()` over
  arrays as the first consumer.
- **Out of scope:** escaping or heap-allocated closures, capturing `this`,
  function values in fields or return types, generic functions, currying, and
  dispatch of any kind.
- **`sort()`:** in place, returning `void`, like `push`, `insert`, `pop` and
  `remove` before it — an array is a shared reference, so a sort that returned a
  new one would invite the reader to think the original was untouched. Stable,
  because predictability is worth more here than the last constant factor.
- **A chosen limit, not a discovered one:** because a function value cannot be
  stored, a callback cannot be kept for later — no handler table, no registry,
  no observer list. Those shapes want a named abstraction, which is what the
  interface milestone is for. Anyone who reaches this wall should find it
  written down here rather than in a compiler error.
- **Validated:** `tests/pass/function_values` covers lambdas bound to locals
  and written in place, a declared function used as a value, inferred parameter
  types, both result spellings, scalar and managed captures, nested lambdas and
  a parenthesised condition that must not read as one. `tests/pass/array_sort`
  covers sorting ints, strings and classes, stability, and the empty and
  single-element cases. Six `tests/fail` fixtures pin each way a function value
  can try to escape, plus capturing a `var` and calling with the wrong
  signature, and `tests/trap/sort_mutation` pins the comparator that mutates
  the array it is sorting.
- **Risk closed:** a comparator can reach the array it is sorting, so the sort
  runs on a snapshot and aborts if the array changed by the end, instead of
  merging out of a buffer that was reallocated underneath it.

## M9 — Sockets, HTTP and the long-range target — Implemented

```skuld
let response = http.get("http://127.0.0.1:8080/data.json") else reason {
    print(http.describe(reason))
    return
}
let document = json.parse(response.body) else reason {
    print(json.describe(reason))
    return
}
```

The target that motivated the whole ordering, spent at last, and by then not a
language milestone: an FFI binding over libc sockets, a response parser in
Skuld, and the JSON parser that closes M4.

- **Implemented:** `std/net` is a blocking TCP connection over libc, `std/http`
  an HTTP/1.1 client on top of it, and `std/json` is M4's parser promoted out
  of its fixture. A Skuld program fetches a document and decodes it.
- **The example changed, and the change is the finding.** It was written with a
  host name. `AGENTS.md` puts reading through a pointer out of scope at the
  foreign boundary, and `getaddrinfo` and `gethostbyname` both answer with a
  pointer to a structure, so no resolver in libc is reachable: a connection is
  made to an address. `http.get` refuses a name by name rather than failing
  somewhere deeper. Reaching names needs a read primitive at the boundary or a
  resolver written in Skuld over UDP, and each is its own decision.
- **`errno` is unreachable for the same reason**, so a network failure says
  which step failed and not why.
- **Out of scope, and still out:** TLS, so plain HTTP only; a server; async,
  non-blocking I/O and any event loop; connection pooling; HTTP/2.
- **Open risk, unchanged:** TLS is where this stops. Binding a system TLS
  library is a milestone of its own and arguably a dependency policy decision,
  not a technical one. `https://` is refused by name rather than attempted.
- **Validated:** `cli/tests/network.rs` binds an ephemeral loopback port,
  answers one request and stops, asserting both the program's output and the
  request the client produced. `tests/pass/std_http_json` covers the library
  halves without a socket, so they run under the sanitizers with everything
  else. Nothing in the suite touches the network.

## M10 — Interfaces — Implemented

The named abstraction every other milestone deferred, and the answer to the
one limit M8 chose: a function value cannot be stored, so a handler table, a
registry or an observer list has had nowhere to live.

```skuld
pub interface Renderer {
    render(value: int) -> string
}

pub class Decimal: Renderer {
    prefix: string

    render(value: int) -> string {
        return "${this.prefix}${value}"
    }
}

func show(renderer: Renderer, value: int) {
    print(renderer.render(value))
}
```

- **Decision — conformance is declared, not inferred.** `class User: Printable`
  states it on the class. Go's implicit satisfaction was considered and
  declined for the same reason a capital letter was declined as an export
  marker in M6: this language says what it means on the declaration. Rust's
  `impl Trait for Type` was declined for putting the relationship in a third
  place, away from both the type and the interface.
- **Decision — only classes implement interfaces.** A struct is a value with
  no identity; putting one behind an interface means boxing it, which means an
  allocation and a lifetime question the milestone does not need to answer. A
  class is already a counted reference, so an interface value is that reference
  plus a table, and reference counting works unchanged.
- **Decision — interface values are storable**, which is the whole point.
  They are class references, so they may be fields, array elements, payloads
  and return types like any other. That restores what M8 left out, and it
  brings the cycles classes already have: `weak` remains the answer.
- **In scope:** interface declaration with `pub`, declared conformance checked
  method by method, an interface as a type anywhere a class may appear, and
  dynamic dispatch.
- **Out of scope:** inheritance between interfaces, default method bodies,
  structs behind interfaces, generics of any kind, and asking at run time which
  concrete class is inside — a downcast needs a decision of its own.
- **Validated:** `tests/pass/interfaces` covers dispatch, a class implementing
  two interfaces and keeping methods beyond them, interface values in an array,
  a field and an `Option` payload, and a registry that outlives the call that
  filled it. Five `tests/fail` fixtures pin the refusals: a struct, a missing
  method, a differing signature, a class that never declared conformance, and a
  method the interface does not name.
- **One thing composed that had to:** a class widens into an interface the way
  a value wraps into an expected `Option`, and the two chain in that order, so
  `Option<Handler>` takes a class directly. It is the only place two implicit
  conversions meet.
- **Cheap by construction:** the shared allocation header already carries a
  `destroy` pointer, so retain and release over an interface value need no new
  runtime code. Dispatch goes through per-class thunks, the same shape M8's
  function values already use, so no call is made through a mismatched function
  pointer type.

## A parallel track — tooling — Partly implemented

Not sequenced with the milestones above and not blocking any of them.

- **Editor highlighting — implemented.** `editors/nvim` is a Vim syntax file
  and filetype detection for `.skuld`. It mirrors the lexer rather than the
  roadmap: the keyword list comes from `Lexer::identifier`, the prelude from
  the resolver, and the escape set is the exact one the lexer accepts, so an
  invalid escape is shown as the error `E0004` would report. It carries no
  semantic knowledge; a capitalised name reads as a type by convention only.
- **Language server — implemented: diagnostics, completion, hover, definition.** `lsp/` is the `skuld-lsp`
  binary: LSP over stdio, reporting the diagnostics the checker produces and
  nothing else. The protocol is written in the crate rather than taken from a
  dependency, so the workspace still has none — the same choice rust-analyzer,
  clangd and gopls each made, and `tower-lsp` was declined for bringing an
  async runtime a synchronous server has no use for.
  - **Decision taken:** full document sync, not incremental. Applying ranges to
    a mirrored buffer is a known source of drift for no gain at this file size.
  - **Decision taken:** positions are converted to UTF-16 in the server rather
    than reusing `SourceFile::location`, which counts Unicode scalars. The two
    agree until a line holds an astral character, and then every column after
    it is wrong by one unit per character.
  - **Open buffers beat disk.** An imported module that is open and unsaved is
    compiled as the editor shows it, through the `ModuleLoader` the compiler
    already takes.
  - **A known limit, not a bug to file:** the pipeline stops at the first stage
    that fails, so a file with both a resolver error and a type error reports
    only the resolver's until that one is fixed. That is the compiler's shape,
    and changing it is a compiler decision, not a server one.
  - **A file is not a program.** Whether an entrypoint exists is a property of
    a program, and an editor showing one file cannot know which program it
    belongs to, so `E0109` is suppressed for a file that declares no `main` of
    its own — otherwise every module file and everything under `std/` is
    permanently red. It is the only diagnostic in the set that needs this.
  - **Every open document is re-checked on any change**, so editing a module
    refreshes the files that import it. A dependency graph would avoid some
    work; at microseconds per check it would cost more than it saves.
  - **Completion answers from the last check that succeeded.** It is wanted
    exactly when the file does not parse, so there is nothing current to ask;
    the tables are a moment stale and the text before the cursor is not. A
    document that never checked offers keywords only. After a `.` the receiver
    decides: enum variants, a module's exports, or the fields and methods of a
    value's type, the builtin ones included. Filtering is the client's, not
    the server's, or the two disagree.
  - **Hover and definition share one lookup** from a position to a symbol, a
    member or a type; only the answer differs. A declaration and a use answer
    alike. The file a declaration lives in is not in the checker's tables — a
    type knows its module, not its file — so it is found in the syntax the
    program was loaded from, the only record of where something was written.
    The prelude has a hover and no definition: it belongs to the language.
- **Still planned.** Find-references and rename, which want a use table read
  the other way round, and formatting, which wants the syntax to stop moving.
  `LANGUAGE.md` names
  `fmt`, `test`, `doc` and `new` as future commands; the official formatter in
  particular has been deferred since the beginning and can land whenever the
  syntax stops moving.

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
4. ~~Callback and lambda syntax?~~ Answered by M8, which the user delegated:
   `func(a: int, b: int) -> int { ... }` as the literal and `func(int, int) -> int`
   as the type, with the types omissible under a known expected type. The
   sketch's `=>` is not adopted.
5. ~~JSON objects as a list of key/value fields, or waiting for a real map
   type?~~ Answered by M4's closing marker: a list of key/value pairs in source
   order, with linear lookup and duplicate keys preserved. Waiting for a map
   would have made the milestone unclosable, since a hash map is a milestone of
   its own and is not authorized. Whether the parser M9 reuses keeps that
   representation is open again once a map exists.
6. ~~Where module boundaries sit — file or directory, `pub` or convention?~~
   Answered for M6 by the user: a module is a **directory**, export is an
   explicit **`pub`**, use is always **qualified** (`json.parse`), and an import
   cycle is an error. M6 settled the implementation question too: the resolver's
   tables grew a file dimension rather than gaining a table above them. Where a
   module path is **rooted** was left open by M6 and answered separately by the
   user for M7: a reserved `std` prefix over sources embedded in the compiler
   binary, with every other path still relative to the program root.
7. ~~Whether function values may capture?~~ Answered by M8: they may, by value,
   and in exchange a function value may not escape into any managed location. A
   closure nothing managed can reach cannot be half of a cycle, so capture costs
   nothing the reference counter has to collect — and a non-escaping function
   value needs no allocation at all.
8. ~~Whether interfaces are part of the callback milestone or a milestone of
   their own?~~ Answered: their own, M10. Their receiver syntax is settled with
   them: a method on a class already takes an implicit `this`, and an interface
   declares the same signature without a body, so nothing new is spelled.
9. ~~Where NUL-terminated C strings are built — a language helper, or a
   standard library function over `[]u8`?~~ Answered by M7: `std/cstring.to_c`,
   in the library. The language keeps knowing nothing about C's terminator, and
   a string that already contains a NUL is refused rather than truncated.
