# Skuld roadmap

This document records completed milestones and a **proposed** next sequence.
M1–M18 are implemented and finished the sequence that built an applications
language. M19–M26 propose the next one, towards the language a system could be
written in; every one of them is **Planned**, which means proposed and not
authorized. No implementation milestone is active, and starting one still
needs an explicit decision recorded in `AGENTS.md`. See `LANGUAGE.md` for
semantics and `README.md` for usage.

Both targets this document set are reached: an HTTP document fetched and
decoded, and a native command-line application that reads local input,
transforms JSON, has its own tests and fetches over HTTPS by host name. The
performance goal has measurements behind it in `BENCHMARKS.md` rather than an
expectation.

## Current baseline

- **Implemented:** M1–M18, including non-escaping lambdas, stable array sorting,
  storable class interfaces, modules, the embedded library, blocking HTTP, the
  official formatter (`skuld fmt` with `--check`), verified rename in the
  editor, whole-file reads and writes, the process arguments, `skuld test`,
  field defaults at construction, a string-keyed map, a DNS resolver written in
  Skuld, system error reasons, verified TLS, and measured performance on two
  native targets.
  `let ... else` also unwraps Option/Result without nesting the success path.
- **Tooling implemented:** highlighting, the official formatter `skuld fmt`, and
  a third workspace crate, `lsp/`, with diagnostics, completion, hover,
  definition, find-references and rename.
- **Current limits:** no user-defined constructor with a body, so a field
  default is an expression and not a step that can fail; no general generics,
  and the map is string-keyed with integer values rather than a general
  collection; a file API that is whole-file and by path only. Name resolution
  is IPv4 A records over UDP, without a cache. TLS is OpenSSL through the FFI
  and is opt-in at link time. x86-64 and i686 Linux are the tested native
  targets, and performance is measured in `BENCHMARKS.md` rather than claimed.
  The limits that define the next sequence are lower down: no bit operators,
  no hexadecimal or binary literals, no named constant, no `usize`, no
  fixed-size array, no reading through a pointer, no layout control, and no
  program without the reference-counting runtime.
- **Syntax reference:** `test.skuld` remains an untouched design sketch.
  Its global statements and incomplete `new User()` are unsupported, and its
  `=>` sorting callback was superseded by M8's `(a, b): int { ... }` form.
  No proposed milestone adds `undefined` or implicit zero values.

The assessment above follows source, fixtures and documentation.

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
for i in 0..bytes.len() { ... }
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
- **Language server — implemented.** Diagnostics, completion, hover, definition,
  references, rename, document highlights, the outline, formatting, signature
  help, inlay hints and semantic tokens. `lsp/` is the `skuld-lsp`
  binary: LSP over stdio, speaking the protocol and asking the compiler. The protocol is written in the crate rather than taken from a
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
- **References and rename — implemented, as M12 below.** They read the use
  table the other way round, and the safety they need is described there.
  - **A highlight is that table narrowed to one document**, and widened in what
    it accepts: a prelude binding and an import qualifier are highlighted
    although neither can be renamed. No `kind` is sent, because the table
    records where a name is written and not whether it reads or assigns.
- **The outline and formatting — implemented.** The outline comes from the
  syntax alone, sorted by span because the AST keeps each kind of declaration
  in a list of its own, and falls back to the last text that parsed.
  Formatting is `skuld fmt`'s formatter over the whole document; a file that
  does not parse or is already formatted is answered with no edits rather than
  an error, since the caller is usually format-on-save. Range formatting is not
  offered: the formatter reads a program, not a fragment.
- **Signature help — implemented.** The call is found over the text, not the
  syntax tree, for completion's reason: a signature is wanted exactly while the
  call is half-written. Parameter names come from the declaration, which is the
  only place they are written; a prelude binding and a builtin method have
  theirs recovered from the line that describes them. A construction lists the
  fields and marks none active, since it names them.
- **Inlay hints — implemented.** The inferred type of a binding that writes
  none, which in Skuld is the only type a reader cannot see: signatures are
  always explicit. Whether an annotation is written is read from the text after
  the name, sound because a `:` follows a binding only in `let name: Type`.
- **Semantic tokens — implemented.** Names only, layered over the editor's own
  highlighting rather than replacing it, which is what the syntax file cannot
  do: a class against a struct, a `let` against a `var`, a method of the
  language against a name from this file. A name the check cannot place is left
  out rather than guessed at. Full document each time; a delta would mean
  keeping the previous stream per document to diff against.
- **Still planned.** `LANGUAGE.md` names `test`, `doc` and `new` as future
  commands; `fmt` arrived with M11.

## Next sequence — Planned, not selected

The numbers express a recommended order, not a requirement to implement every
entry. M11 and M12 improved daily use without new language semantics. M13 was the
first application milestone. M14 and M15 addressed language ergonomics separately;
M16 and M17 took the foreign-boundary and dependency decisions they needed.
M18 can collect a baseline earlier, but optimizations must follow measurements.

| Milestone | Deliverable | Dependency |
| --- | --- | --- |
| M11 | Official formatter | Implemented |
| M12 | References and safe rename in the LSP | Implemented |
| M13 | Local CLI applications and `skuld test` | Implemented |
| M14 | Field defaults and construction | Implemented |
| M15 | A map for real application data | Implemented |
| M16 | Host names and useful network errors | Implemented |
| M17 | Verified HTTPS | Implemented |
| M18 | Performance baseline and a second native target | Implemented |

## M11 — Official formatter — Implemented

The tool that makes Skuld code consistent and keeps review discussions on behavior.

- **Implemented:** `skuld fmt <file>` formatting in-place and `skuld fmt --check <file>`
  reporting drift with a non-zero exit code without modifying the file. Preserves
  all `//` comments (leading and trailing), literal values with original formatting
  and escapes, statement boundaries, multiline expressions, and AST semantics.
- **Decision taken — return type normalization:** function and method return
  signatures normalize to `-> Type`. Both `:` and `->` remain fully accepted by the
  language and parser; formatting standardizes the representation across the codebase.
  Method signatures on interfaces and extern blocks normalize to `-> Type`.
- **Decision taken — indentation and spacing:** 4-space indentation; binary operators
  and assignments padded with single spaces; comma-separated lists padded after commas;
  colons in typed parameters and field declarations padded after the colon (`name: Type`).
- **Decision taken — match arms:** arms normalize to `Pattern: stmt` / `Pattern: { ... }`.
  Enum variants formatted one per line with trailing commas.
- **Decision taken — blocks and control flow:** `{` sits on the opening line; `}` on
  its own line aligned with the parent; `} else {` stitched cleanly; empty blocks
  compact to `{}`.
- **Closing marker — reached:** formatting all 17 executable examples and 57 pass
  fixtures is idempotent (`format(format(x)) == format(x)`), preserves native execution
  output byte-for-byte under clang, check mode reports drift without writes, and invalid
  input produces diagnostics without overwriting the file. `test.skuld` is preserved
  untouched. Integration tests in `cli/tests/fmt.rs` and `cli/tests/cli.rs`.
- **Fixed after the milestone:** two faults that inserted blank lines the
  author had not written. The separator between declarations was emitted after
  the next declaration's comments, detaching a comment from what it documents,
  and the gap before a comment was measured from the previous comment rather
  than from the last thing emitted, so a note above the last variant of an enum
  inherited a gap that was never there. A comment the author left standing
  alone still keeps its blank line on both sides.
- **Out of scope, and still out:** syntax changes, import reorganization, and lint rules.

## M12 — References and rename — Implemented

Refactoring a multi-module program from the editor, without reading every file
to be sure nothing was missed.

- **Implemented:** `textDocument/references` over the declaration and every
  resolved use, with `includeDeclaration` honoured; `textDocument/prepareRename`
  answering the range and placeholder or the reason for a refusal; and
  `textDocument/rename` returning a workspace edit across every file of every
  program the server has checked. Offsets are the compiler's bytes and the
  ranges are UTF-16, like every other answer.
- **Decision taken — what a workspace is.** The server is told about documents,
  not about a directory tree, and a Skuld program is identified by its entry
  file. So the searched set is the programs the editor has open: each document
  is compiled as the entry of its own program, and a module's uses are found
  through whichever entry file reaches it. A program nothing open reaches is not
  searched, and inventing a root by scanning directories would have meant the
  server reading files nobody asked it to open. Closing a document forgets its
  check, so a reference search never answers out of a text nobody is looking at.
- **Decision taken — the initial set of renameable symbols.** Functions,
  parameters and locals, which are exactly the names the resolver's tables
  record. A prelude binding, an import qualifier — the last segment of a
  directory path — a struct, class or enum name, a field, a method, and anything
  declared in the embedded standard library are each refused with the reason
  they are refused. Type names are the notable gap: they live in the checker's
  own namespace and their uses in annotations are not in the use table, so a
  rename would leave the annotations behind. Moving them means recording type
  uses, which is a compiler change and not a server one.
- **Decision taken — a rename is verified, not trusted.** Rewriting the recorded
  uses is the easy half. Every program the edit touches is checked again over
  the edited texts, and where each name resolves is compared before and after,
  offsets shifted by the edits that precede them. A new name that captured a use
  from an outer scope, or lost one to an inner scope, is refused with its reason
  even though the result would compile. A buffer that has changed since it last
  checked is refused too, because its recorded offsets are positions in a text
  that no longer exists.
- **Closing marker — reached:** renaming `origin` in `tests/pass/modules/geometry`
  from the file that imports it edits both files at once and writes neither —
  a workspace edit is the client's to apply. Tests cover unsaved buffers, an
  astral character on the edited line, a collision the checker reports, a
  capture the checker would not, a stale buffer, invalid source, and the
  refusals above.
- **Out of scope, and still out:** incremental compilation, automatic fixes,
  editor-specific UI, renaming a type or a member, and a workspace the editor
  has not opened.

## M13 — Local CLI applications and tests — Implemented

The first milestone whose deliverable is a program rather than a language
feature: `examples/jsontool.skuld` reads a JSON file, selects part of it by a
dotted path and writes the result, and `skuld test` runs its suite.

- **Implemented:** `std/fs` (`read_file`, `read_text`, `write_file`,
  `write_text`), `std/os` (`arguments`, `parameters`, `flush`, `exit`),
  `std/testing` (`check`, `equal_int`, `equal_text`, `equal_bool`, `fail`,
  `passed`), the `skuld test` command, and the example application with a
  module of its own and seven tests.
- **Decision taken — files are opened by path, not by handle.** A handle
  exposed to Skuld would need a lifetime rule: who closes it, and what happens
  to one that is dropped. Skuld has no destructor a user can write, and a file
  read or replaced whole — which is what a document transformer does — needs
  none. The cost is that streaming a file larger than memory is not possible
  yet, and it will need the handle question answered.
- **Decision taken — the process bridge lives in the runtime.** Reading `argv`
  means following a pointer to a pointer, which the foreign boundary refuses,
  so the runtime offers a count, a length, a copy into bytes Skuld already owns
  and an exit that flushes first. It is shaped exactly like the boundary
  already is — scalars and a pointer to bytes the caller owns — and the
  checker's rule that no managed value crosses `extern "C"` is untouched. The
  generated `main` now takes `argc`/`argv` and hands them straight to the
  runtime; that is the only place in a program that sees them.
- **Decision taken — bytes or text is the caller's choice.** `read_file`
  answers `[]u8` and `read_text` answers a `string` after UTF-8 validation, so
  nothing is decoded that did not ask to be.
- **Decision taken — a test is a function, not an annotation.** A top-level
  `func test_...()` taking nothing and returning nothing is a test. A naming
  rule needs nothing from the compiler, and annotations are not this
  milestone's to introduce. A name that looks like a test but cannot be called
  as one is an error, not something skipped: a test that silently never runs is
  the worst outcome available.
- **Decision taken — the suite is one program.** The file is compiled once with
  an entry point the runner writes, and the tests run in source order in one
  process. Skuld has no recoverable panic, so the first failure stops the run
  and the report says which tests never started. Each finished test prints a
  line that is flushed as it is written, so the record survives a trap that
  kills the process. Compiling once per test would make each independent at the
  price of a clang invocation each; that trade can be revisited when a suite is
  large enough to care.
- **Decision taken — `main` still returns void.** A program that wants a status
  calls `os.exit(code)`, which flushes and leaves. Nothing is released on the
  way out, because the process is ending.
- **Decision taken — forwarding has a marker of its own.** `skuld run file
  --args a b c` hands everything after `--args` to the program, unread, `--`
  included. `--` already means "every later argument is a path" in this CLI, so
  redefining it would have broken a documented, tested rule; a second marker
  costs nothing and keeps both meanings. Only `run` executes a program, so only
  `run` accepts it.
- **Closing marker — reached:** `cli/tests/jsontool.rs` builds the application,
  reads a temporary JSON file, writes a selected part to another file and
  checks both, and covers a missing file, malformed input, a path that leads
  nowhere and a command line that makes no sense — each with its own exit
  status, and nothing written when the run fails. `cli/tests/testing.rs` runs
  the application's own suite through `skuld test`, and covers a failing
  assertion, a trap, a file with no tests, a file that declares `main` and one
  that does not compile. Every temporary file belongs to the host test and is
  removed with the scratch directory.
- **Out of scope, and still out:** subprocess execution, directory traversal, a
  package registry, a project generator, a general IO framework, `errno`
  detail, and any file API that is not whole-file.

## M14 — Field defaults and construction — Implemented

```skuld
class Account {
    owner: string = "unnamed"
    tags: []string = []
    balance: int = 0
}

let blank = new Account()
let ada = new Account(owner: "Ada", balance: 120)
```

- **Implemented:** `name: Type = expression` on a field of a class or a struct;
  a field with a default may be left out of `new C(...)` and of `S { ... }`;
  the default is checked once against the field's type where it is written, and
  evaluated at every construction.
- **Decision taken — a default is an expression, not a constructor.** There is
  no `init` block with a body. A constructor would mean a partially
  initialized `this` — a reference to an object whose fields do not all have
  values yet — and that is the one thing construction in Skuld has always
  guaranteed against. Defaults keep the guarantee for free: every field either
  carries one or must be supplied. What this costs is initialization that can
  fail, which would need a constructor returning a `Result`; that is a decision
  of its own, and a function returning `Result<Account, E>` is already the way
  to write it.
- **Decision taken — a default sees the file, not the object.** It resolves in
  the scope of the file that declares the type, so neither `this` nor a sibling
  field is in scope — there is no object at the point a default runs. Both are
  what a reader reaches for first, so an unknown name in a default says exactly
  that instead of the generic spelling advice.
- **Decision taken — order.** The arguments that were written are evaluated
  first, left to right, exactly as everywhere else in the language; then the
  defaults of the fields nobody wrote, in declaration order. Evaluating
  everything in field order would have reordered the arguments a caller wrote,
  which is the one rule `AGENTS.md` will not trade.
- **Decision taken — structs too.** The check and the lowering step are the
  same for both, and leaving value types out would have been an arbitrary seam
  in the language rather than a simplification.
- **Closing marker — reached:** `examples/defaults.skuld` and
  `tests/pass/field_defaults.skuld` cover defaulted and supplied fields, a
  managed default per object, and the evaluation order, under the address,
  leak and UB sanitizers with the rest of `tests/pass`. The three
  `tests/fail/field_default_*` fixtures hold the diagnostics: a default of the
  wrong type, one reaching for `this` or a sibling field, and a field without a
  default left out of a construction.
- **Out of scope, and still out:** overloads, inheritance, global statements,
  zero or null defaults, a constructor body, and initialization that fails.

## M15 — Maps with an application consumer — Implemented

- **Decision taken — a concrete library collection, not a builtin and not
  generics.** Generics are a milestone of their own that nothing has
  authorized, and a builtin map would have put a second collection in the
  compiler for one consumer. `std/map` is therefore a `StringMap`: a map from
  `string` to `int`, written in Skuld. The value is a position, a count or an
  identifier, and what is being indexed stays in the array that already holds
  it — which is how a JSON object keeps its order and its duplicates and is
  still searchable.
- **Implemented:** `set` (answering what it replaced), `get`, `has`, `remove`,
  `len`, `keys`, `values`, open addressing with linear probing, and geometric
  growth at 70% occupancy counting removed entries.
- **Decision taken — iteration is insertion order.** A hash table has no order
  of its own, so the choice was between whatever the buckets hold — which
  changes at every resize — and the order the caller built. Predictable output
  is worth the key list it costs. A removed key keeps its slot so the rest of
  that order is untouched, and a rehash is where removed entries are really
  dropped. A key removed and set again keeps its original place, because it
  keeps its original entry.
- **Decision taken — a missing key is an `Option`,** which is what the language
  already says about absence; there is no zero value to return instead.
- **Equality and hashing:** keys compare by their bytes, and the hash is a
  polynomial over them, kept under 2^30 so it cannot overflow an `int`. Skuld
  has no bitwise operators, so FNV-1a — which needs a xor — was not available.
  It is not meant to resist a hostile key set, and a user-supplied hash is out
  of scope.
- **JSON is untouched:** its members stay a list in source order with
  duplicates preserved, exactly as M4 decided.
- **Closing marker — reached:** `jsontool --keys` counts every member name in a
  document, which is a lookup per member and quadratic through an array.
  Measured with `tests/bench/map_lookup.skuld` on the machine that wrote this,
  20000 keys take **1896 ms** through the parallel-array search and **21 ms**
  through the map; at 2000 keys it is 26 ms against 3 ms.
  `tests/pass/string_map.skuld` covers replacement, removal, growth across
  several rehashes, collisions and a missing key, under the address, leak and
  UB sanitizers with the rest of `tests/pass`.
- **Out of scope, and still out:** sets, user hash implementations, a
  collection hierarchy, and any key type but `string`.

## M16 — Host names and network error detail — Implemented

- **Decision taken — a resolver in Skuld over UDP.** The gate was between a
  foreign-memory primitive, a native bridge and this. Every resolver in libc
  answers with a pointer to a structure, and a primitive that read through one
  would have weakened the single rule that makes this FFI safe, for one caller.
  `std/dns` is a DNS client written in Skuld: a UDP socket `connect`ed to the
  server, so `send` and `recv` suffice and `recvfrom`'s address-out parameter
  is never needed.
- **Decision taken — `errno` is a runtime bridge, separately.** The UDP choice
  says nothing about error detail. `errno` is a macro over a function returning
  a pointer and `strerror` answers with one, so `sk_errno` and
  `sk_error_message` read them in the runtime and hand back a number and bytes,
  exactly as M13's argument bridge does. `std/fs` and `std/net` now carry an
  `OsFailure` — what was attempted and the system's number — so a refused
  connection reads "could not connect to 127.0.0.1:9: Connection refused".
- **Scope taken:** IPv4 A records, servers from `/etc/resolv.conf` in order, a
  three-second receive timeout so an unreachable server fails rather than
  hangs, and a literal address resolved as itself without asking anyone. A
  name that does not exist is final; a transport failure moves to the next
  server.
- **Still provisional:** there is no random source in the language, so the
  query id comes from the clock. With the socket's ephemeral port it is what an
  answer has to match, and that is not a defence against an off-path attacker.
  It is meant for the nameserver the machine already trusts.
- **Closing marker — reached:** `cli/tests/resolver.rs` runs a DNS server on a
  loopback port of its own and an HTTP server beside it: a name resolves
  against the first and the address that comes back is what the request goes
  to. It also covers a refused name, a literal, a server that never answers and
  a label longer than the protocol allows. Nothing in the suite touches public
  DNS.
- **Out of scope, and still out:** IPv6, CNAME chasing, a cache, `search`
  domains, TCP fallback for truncated answers, and servers, async IO or
  unrestricted FFI expansion.

## M17 — HTTPS with certificate verification — Implemented

- **Decision taken — OpenSSL, through the FFI.** Custom cryptography is out of
  the question and nothing authorizes it, so a system library was the only
  honest option. Supported versions are whatever the linked OpenSSL negotiates
  through `TLS_client_method`.
- **Decision taken — the dependency is quarantined, not spread.** `std/tls`
  and `std/https` are their own modules; `std/http` still refuses `https://`
  and links nothing. A program that wants TLS imports it and says so:
  `skuld build fetch.skuld -lssl -lcrypto`. That keeps the compiler free of
  dependencies and makes the one a program takes visible in its build command.
- **Decision taken — verification cannot be turned off.** There is no
  `insecure` flag, because the first thing anybody would do with one is reach
  for it. The chain is checked against the system trust store
  (`SSL_CTX_set_default_verify_paths`), the name against the certificate
  (`SSL_set1_host` before the handshake and the verification result read
  after), and SNI is sent so the right certificate arrives.
- **Decision taken — two pointer operations were needed and no more.**
  `std/ffi` has `null()` and `is_null()`: a library that allocates answers with
  NULL and one that takes an optional callback wants NULL. Neither reads
  through a pointer, so dereferencing and arithmetic stay out.
- **Ownership:** a connection owns its socket, its `SSL` and its `SSL_CTX`, and
  `close()` releases them in that order. Every failure on the way in releases
  what it had already taken; the whole path runs clean under the address and
  leak sanitizers.
- **Closing marker — reached:** `cli/tests/https.rs` creates a certificate
  authority, signs a certificate for `localhost` with it, runs
  `openssl s_server` on a loopback port and fetches JSON over the verified
  connection, parsing it with `std/json`. It refuses an untrusted chain, a
  certificate for another name and an expired one, each with its own message.
  No test disables verification and none reaches the public network.
- **Out of scope, and still out:** HTTP/2, connection pools, async, TLS
  servers, client certificates, redirects, and any test against a public
  endpoint.

## M18 — Measured performance and portability — Implemented

- **Decision taken — the second target is i686-linux-gnu.** It was chosen
  because it can be *run* here and not only built for: a claim about ARM or
  macOS with no machine behind it would be a claim rather than evidence, and a
  32-bit pointer is what finds the places that assumed a 64-bit one. Bringing
  up a third target is its own decision, and `BENCHMARKS.md` lists what it
  would have to settle.
- **Decision taken — a benchmark is only a comparison if the checksums
  agree.** Every program prints a number derived from its whole result before
  it is timed, and the Skuld, Rust and Go versions must print the same one.
  The Rust and Go versions are therefore written the way the Skuld one is —
  same generator, same loop, same comparison — rather than in the most
  idiomatic form of their language: the subject is a language's cost for a
  given piece of work.
- **Workloads:** array growth with a stable sort through a comparator; building
  a string byte by byte and scanning it; dynamic dispatch through an interface;
  a string-keyed index against the array search it replaced; and parsing a
  61 KB JSON document. Compilation is measured separately.
- **Differences that could not be aligned, and are reported:** `json_parse` has
  no Rust version, because Rust's standard library has no JSON and a crate
  would compare Skuld's library against a third party rather than a language;
  Go's uses `encoding/json` into `map[string]any`, which is comparable work and
  not the same code.
- **Results** (Xeon E5-2640 v3, clang 22 `-O2`, rustc 1.98 `-O`, Go 1.27):
  arrays 37 ms against 17/209; strings 103 ms against 50/63; dispatch 84 ms
  against 65/150; JSON 211 ms against Go's 470; map 12 ms against 7/6. Between
  **1.3x and 2.2x** the faster of Rust and Go, and faster than Go on two
  workloads. The full report, including peak memory and the compiler's own
  timings, is in [`BENCHMARKS.md`](BENCHMARKS.md).
- **Found by measuring, and fixed:** every read of a managed local took a
  counted copy of it for the length of an expression — a retain and a release
  per index, per field access, per comparison. A binding nothing can reassign
  already holds that reference, so the read borrows it now — and so does a read
  through a field of one, where the index runs no code that could replace it.
  JSON went from 582 ms to 211 ms and a byte scan from 433 ms to 121 ms, with
  `tests/pass/borrowed_reads.skuld` added to keep it safe. Iterating a string's
  bytes no longer builds the array `bytes()` would answer, either, and a read
  no longer checks its index twice.
- **Found by measuring, and left alone:** the compiler's share of a build is
  under 1% — 4 ms to check a 646-line file against 871 ms for the build — so
  the edit-compile loop is a question of what is asked of clang, not of the
  compiler. Strings remain the weakest workload, paying a bounds check per push
  with no way to reserve capacity; that is where the next optimisation would
  start, with before-and-after evidence.
- **Closing marker — reached:** `tests/bench/` holds the programs, the checked-in
  input and `run.sh`; `BENCHMARKS.md` is the report; and
  `cli/tests/portability.rs` builds and runs every `tests/pass` fixture on the
  second target, where **58 of 59** produce identical output. The exception,
  `extern_c_ffi`, is the one place a target leaks into the language: an
  `extern "C"` declaration names a concrete width and C's own types do not, so
  `u64` for a `size_t` is right on x86-64 and wrong on i686. Skuld has no
  `usize`, and adding one is a language decision, not a portability fix.
- **Out of scope, and still out:** a promised speed ratio, replacing the C
  backend, changing copy semantics without evidence, threading, and a new
  memory-management model.

## The systems sequence — M19–M26 — Proposed

The first eighteen milestones built an applications language: it fetches a
document, parses it, writes a file and runs its own tests, and it does that
within a few multiples of Rust and Go. The stated ambition is lower than that —
a language a system could be written in, down to an operating system — and the
gap is not a matter of libraries. What follows is the assessment that produced
this sequence, and then the milestones.

**What a systems language needs that Skuld has:** value structs with a
predictable layout, sized integers at fixed widths, arithmetic that traps
rather than wrapping silently, no GC, a foreign boundary, modules, and
compilation to native code through a backend that already produces competitive
output.

**What it needs that Skuld does not have, in the order the gaps bite:**

1. **No bit operations at all.** `&`, `|`, `^`, `~`, `<<` and `>>` are not
   operators; `&` is *an invalid character*. No mask, no flag set, no packed
   field, no checksum, no hash written by hand. This is the single largest gap
   and nothing below matters as much.
2. **No literal for the values that bits are written in.** `0xFF`, `0b1010`,
   `0o755` and `1_000_000` are all lexical errors, so permissions, masks,
   opcodes and addresses are written in decimal or not at all.
3. **No named constant anywhere.** There is no `const`, and a declaration at
   module level can only be a function or a type. Every protocol number in
   `std/net` is a bare literal in the call that uses it, which is exactly the
   style a systems language must not force.
4. **No word-sized integer.** `usize`/`isize` do not exist, which M18 already
   found from the other side: `size_t` has no correct spelling in an
   `extern "C"` declaration, so the FFI fixture is the one thing that does not
   port to i686.
5. **No memory that is not a heap allocation.** Arrays are heap-allocated and
   dynamic; there is no `[N]T`, so there is no buffer on the stack and no
   struct that embeds a fixed number of bytes.
6. **No reading through a pointer.** `*T` exists only to hand a borrowed buffer
   to C. Nothing loads or stores through one, and there is no pointer
   arithmetic. That rule is what makes today's FFI safe, and it also makes an
   allocator, a device register, a page table or a linked list impossible.
7. **No layout control.** No packing, no alignment, no unions, no explicit
   discriminant on an enum, and no struct crossing the ABI by value. A
   hardware or kernel structure cannot be described.
8. **No program without the runtime.** Every program carries reference
   counting and links libc; there is no freestanding mode, no `static`
   mutable state, no way to supply an allocator, and no entry point other
   than the generated `main`.
9. **No `defer`.** Reference counting releases what it owns, which is most
   things today and none of the things an `unsafe` block would own.
10. **No integer/float conversion, and no `char`.** `float(3)` is rejected and
    `'a'` is not a value, so a byte comparison is written against a decimal
    number.

**Two syntax debts to settle while doing the above**, both of which are two
ways to say one thing: a return type is written `-> Type` or `: Type` and both
are accepted, with the formatter normalising to the first; and a lambda's
parameter list plus a method's declaration mean `(a: int): int { ... }` is a
lambda while `hello(): int { ... }` is a method, told apart only by where they
appear. The first should be settled by removing `:` from return position once
the formatter has rewritten the fixtures; the second is a consequence of
methods carrying no `func`, which is the user's decision and stays.

**What deliberately does not change.** No GC, no borrow checker, no ownership
system, no inheritance, no exceptions, no `null`, no implicit coercion between
widths, and no wrapping arithmetic by default. Reference counting stays the
memory model for the language that exists; the freestanding subset below is
how a program opts out of it, not a second memory model bolted onto the first.

## M19 — Bits and integer literals — Planned

The floor everything else stands on. Without it Skuld cannot express the
values a system is made of.

```skuld
let flags = 0b0000_1101
let mask: u32 = 0xFFFF_0000
let permissions = 0o644
let high = (value & mask) >> 16
let set = flags | FLAG_READ
let cleared = flags & ~FLAG_WRITE
```

- **In:** `&`, `|`, `^`, `~`, `<<`, `>>` and their compound forms; `0x`, `0b`
  and `0o` literals; `_` as a digit separator in every base.
- **Semantics to settle in the milestone:** shifts are defined at every width,
  and a shift by more than the width is a trap rather than C's undefined
  behaviour. `>>` on a signed type is arithmetic and on an unsigned type is
  logical, which follows from the type and needs no second operator. Bit
  operations never mix widths, exactly like arithmetic.
- **Open decision — the lexer's `>>`.** Today `>>` is deliberately two closing
  tokens so `Option<Result<int, string>>` needs no special rule. A shift
  operator meets that head on. The cheapest answer is that the type grammar
  closes generics one `>` at a time and the expression grammar reads `>>` as
  one token, which is a parser rule rather than a lexer mode; that has to be
  decided and written down.
- **Closing marker:** `std/utf8` and `std/json` drop their arithmetic
  workarounds for bit work, and a fixture computes a CRC32 and an FNV-1a hash
  in pure Skuld with checked results.
- **Out:** rotate operators, saturating or wrapping operators, and bitfield
  syntax on struct fields.

## M20 — Constants and value patterns — Planned

A named constant is what makes the numbers in M19 readable, and once values
have names they belong in patterns too.

```skuld
pub const AF_INET: i32 = 2
pub const SOCK_STREAM: i32 = 1

match signal {
    SIGINT: stop()
    SIGTERM: stop()
    1..8: fatal(signal)
    _: ignore(signal)
}
```

- **In:** `const NAME: Type = expression` at module level and inside a
  function, `pub const` as an export, and `match` over integer and string
  values with constant patterns, ranges and a required `_`.
- **Semantics to settle:** a `const` is evaluated at compile time, which means
  the compiler gains a small constant evaluator — literals, the arithmetic and
  bit operators, and other constants. It is not general compile-time
  execution, and the milestone says so explicitly.
- **Open decision — does a `const` of a managed type exist?** A `const string`
  is a literal and costs nothing; a `const []int` would be an allocation that
  has to happen somewhere. The narrow answer is to allow scalars and strings
  only and revisit when something needs more.
- **Closing marker:** `std/net`, `std/dns` and `std/fs` name every protocol
  number and `errno` value they use, and no bare literal is left in a call
  where a constant would say what it means.
- **Out:** generic constants, `const fn`, and constant expressions in type
  position — which is what a fixed-size array wants and M22 has to settle.

## M21 — Word-sized integers and conversions — Planned

The gap M18 found, plus the conversions that today force a program through
strings.

- **In:** `usize` and `isize` as their own types, `size_t`-shaped and
  target-dependent; `float(i)` and `int(f)` as explicit, trapping conversions;
  a `char` value whose literal is `'a'`, with explicit conversion to and from
  `u8` and `u32`.
- **Semantics to settle:** `usize` is a distinct type and not a spelling of
  `u64`, so a program that assumes the two are interchangeable stops compiling
  on the target where they are not. `int(f)` truncates towards zero and traps
  on a value the target type cannot hold, including NaN.
- **Closing marker:** `tests/pass/extern_c_ffi.skuld` uses `usize` where C uses
  `size_t`, and `cli/tests/portability.rs` runs **every** fixture on i686 with
  no exception left.
- **Out:** `f32`, 128-bit integers, and implicit widening anywhere.

## M22 — Fixed-size arrays and stack buffers — Planned

The first milestone about memory rather than about values: a place to put
bytes that is not the heap.

```skuld
var line: [256]u8 = [0; 256]
let header: [4]u8 = [0x7F, 'E', 'L', 'F']
```

- **In:** the type `[N]T` with value semantics and copy on assignment, the
  repeated-element literal, indexing with the same bounds checking `[]T` has,
  `len()` as a compile-time constant, and a fixed array as a struct field so a
  record can embed its own buffer.
- **Semantics to settle:** `N` is a constant expression, which is why M20 comes
  first; a `[N]T` coerces to a `[]T` slice for reading without allocating,
  which is the whole point of having one.
- **Open decision — does a fixed array live in a class field?** A class field
  is reference-counted and a fixed array is not; the narrow answer is yes,
  inline in the object's allocation, with the same rules its other fields
  have.
- **Closing marker:** `std/fs` and `std/net` read into a stack buffer instead
  of pushing into a heap array byte by byte, and the benchmark that builds a
  string shows the difference.
- **Out:** multidimensional array syntax, array-of-array literals with
  inference across dimensions, and a growable buffer type in `std`.

## M23 — Pointers that can be read — Planned

The milestone that makes Skuld a systems language, and the one that has to be
argued rather than assumed: today's FFI is safe precisely because nothing
dereferences a pointer, and this removes that.

```skuld
unsafe {
    let value = load<u32>(register)
    store<u32>(register, value | ENABLE)
    let next = offset(base, index)
}
```

- **In:** typed load and store through `*T` inside an `unsafe` block,
  pointer arithmetic in element units, pointer/integer conversion, a
  `volatile` load and store that the backend may not reorder or elide, and
  taking the address of a local or a fixed array.
- **The rule that must survive:** an `unsafe` block is where the compiler's
  guarantees are suspended *and says so*. Everything outside one keeps every
  rule it has today — no unchecked index, no aliasing of a managed value, no
  pointer at all. The FFI does not become unrestricted: a signature still
  refuses managed types, and `ptr()` still borrows rather than escapes.
- **Open decision — does a pointer read produce a managed value?** Loading a
  `string` or a class reference through a pointer would hand the reference
  counter a value it never saw allocated. The narrow answer is that only
  scalars, fixed arrays of scalars and pointer types can be loaded or stored,
  and a managed type through a pointer is a diagnostic.
- **Open decision — what spells the operation.** `load<T>`/`store<T>` are
  functions and need no syntax; a `*pointer` dereference reads better and
  costs a prefix operator that collides with nothing today. This is the only
  new operator the sequence proposes and it should be decided deliberately.
- **Closing marker:** a bump allocator and an intrusive linked list written in
  Skuld, running clean under the address sanitizer, with the unsafe surface
  confined to a handful of functions.
- **Out:** references with lifetimes, aliasing rules, a borrow checker, and
  unchecked indexing outside `unsafe`.

## M24 — Layout and the ABI — Planned

Describing memory somebody else defined: a hardware register block, a kernel
structure, a file header, a C library's struct.

- **In:** an explicit layout attribute on a struct (packed, and alignment),
  `offset_of`, an enum with explicit discriminant values and an explicit
  underlying integer type, conversion between such an enum and its integer,
  unions, and structs passed to and returned from `extern "C"` by value.
- **Semantics to settle:** the default layout stays the compiler's own and
  unspecified; a struct that crosses the boundary says so. A union is only
  readable inside `unsafe`, because which member is live is the program's
  claim and not the compiler's knowledge.
- **Closing marker:** `std/net` builds `sockaddr_in` and `std/dns` builds
  `struct timeval` as declared types rather than as hand-packed `[]u8`, and
  both are identical on x86-64 and i686.
- **Out:** bitfields with C's allocation rules, `#[repr(Rust)]`-style
  guarantees, and layout that varies by target in the same source.

## M25 — `defer` and deterministic cleanup — Planned

Reference counting releases what it owns, and M23 introduces things it does
not own.

- **In:** `defer statement` running at scope exit in reverse order, including
  on every early return and on a trap-free `break` or `continue`; an
  interaction with `?` and `let ... else` that is specified rather than
  discovered.
- **Open decision — is there an `errdefer`?** Go has one path, Zig has two.
  The narrow answer is one, since `Result` already makes the failure path
  explicit at the call site.
- **Closing marker:** `std/tls` and `std/net` release their handles through
  `defer` instead of repeating the cleanup on every failure path, and the
  sanitizers stay clean.
- **Out:** destructors a user can write, `Drop`-style traits, and cleanup
  attached to a type rather than to a scope.

## M26 — Freestanding Skuld — Planned

The milestone the long-range ambition actually needs: a program with no
runtime and no libc, which is the shape a kernel, a bootloader or a static
utility has.

- **In:** a build mode that emits no reference-counting runtime and links
  nothing, a language subset the checker enforces in that mode (no `string`,
  no `[]T`, no class, no `Option` payload that allocates — what is left is
  scalars, fixed arrays, structs, enums and pointers), `static` mutable state
  with an initialisation rule, a user-supplied entry point, and a build that
  produces an object file rather than an executable.
- **The point:** the memory model does not change. A freestanding program has
  no managed values at all, so reference counting has nothing to do; a hosted
  program keeps exactly what it has today. Two modes, one language, and the
  checker says which one a file is being compiled in.
- **Open decision — how does a program get memory?** An allocator written in
  Skuld against a region the program is given, declared through an interface
  the language knows about, is the shape that keeps the compiler out of it.
  Whether the hosted mode can then use a different allocator is a separate
  question, and a bigger one.
- **Closing marker:** a static Linux x86-64 program with no libc that makes
  `write` and `exit` syscalls directly, and a bootable image that prints to
  the VGA text buffer, both built by `skuld build` and run in CI under QEMU.
- **Out:** interrupts, a scheduler, drivers, a memory manager, and anything
  that is an operating system rather than the language it would be written in.

## Beyond this sequence — Candidates, not selected

General generics remain conditional on a concrete consumer and their own design;
recursive enum boxing, escaping closures, interface inheritance/downcasts,
`skuld new`, `skuld doc`, an HTTP server, async, alternative backends and
self-hosting have no implementation authorization. The systems sequence adds
its own candidates that it deliberately does not include: inline assembly,
atomics and a memory model, threads with an atomic reference count, interrupt
and naked calling conventions, linker sections, and a target that is not
Linux. Each is a milestone of its own, after M26 has shown what a program
without a runtime actually needs. Revisit them after the CLI
application and measurements expose a need, rather than adding them to every
milestone. No package registry, GC or borrow checker is proposed.

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
   `(a: int, b: int): int { ... }` as the literal and `(int, int) -> int`
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
9. How does `>>` coexist with `Option<Result<int, string>>`? The lexer emits
   two closing tokens today precisely so nested builtin generics need no rule.
   M19 has to decide, and the cheapest answer is a parser rule — types close
   one `>` at a time, expressions read `>>` as one operator — rather than a
   lexer mode.
10. Does an `unsafe` block that can read memory undo the argument that makes
    the FFI safe? Today no pointer can be dereferenced anywhere, which is why
    a foreign signature refusing managed types is enough. M23 replaces that
    with a narrower claim: the guarantee holds outside `unsafe`, and only
    scalars, fixed arrays of scalars and pointers may cross a load or a store.
    Whether that is enough has to be argued in the milestone and not assumed.
11. Is the freestanding subset the same language? M26 says yes — one language,
    two build modes, with the checker refusing managed values where there is no
    runtime to manage them. The alternative, a second dialect, is what the
    project should not become, and saying so now is cheaper than discovering it
    later.
12. Does the hosted language ever get a replaceable allocator? A freestanding
    program must supply one. Letting a hosted program do the same touches
    every managed allocation, and needs its own evidence.
13. Does the return type keep two spellings? `-> Type` and `: Type` are both
    accepted and the formatter already writes the first. Removing the second
    is a one-line parser change and a fixture sweep; keeping both is a
    permanent second way to say one thing.
14. ~~Where NUL-terminated C strings are built — a language helper, or a
   standard library function over `[]u8`?~~ Answered by M7: `std/cstring.to_c`,
   in the library. The language keeps knowing nothing about C's terminator, and
   a string that already contains a NUL is refused rather than truncated.
