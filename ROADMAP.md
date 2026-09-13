# Skuld roadmap — Planned

This document records the **proposed** sequence for the milestones after the
current one. Nothing here is implemented, and recording a milestone here is not
authorization to start it: `AGENTS.md` remains the authority on which milestone
is active, and the user selects it explicitly. Read `LANGUAGE.md` for what the
language actually does today and `README.md` for what currently runs.

The long-range target that motivates this ordering is a program that performs
an HTTP request and decodes a JSON response. That target is deliberately **not**
reachable within these five milestones; they build the foundations it needs.
The naming below is local to this document.

## In flight — array growth

`push`, `insert`, `pop` and `remove` on `[]T`, with their `tests/pass`,
`tests/fail` and `tests/trap` fixtures. This work exists in the tree and is not
yet part of the completed status in `AGENTS.md`. Close it before opening M1.

## M1 — Enums and `match`

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
    JsonValue.Text(s) -> print(s)
    JsonValue.Number(n) -> print("${n}")
    _ -> print("other")
}
```

First because it is the largest single unlock: `Result`, `JsonValue` and any
future state machine depend on it. Builtin `Option` already carries an inline
tag/payload representation with conditional retain/release of managed payloads;
M1 generalizes that machinery rather than duplicating it.

- **In scope:** declaration, a type namespace alongside structs, variant
  construction, exhaustiveness checking, immutable payload bindings per arm,
  correct reference counting per variant.
- **Out of scope:** guards, nested patterns, or-patterns, explicit
  discriminants.
- **Open risk:** recursive variants such as `Items([]JsonValue)` force an
  indirect representation. Decide early between automatic boxing of recursive
  variants and requiring the user to go through a class or array.
- **Validation:** non-exhaustive `match` fixtures in `tests/fail`; payload
  reference counting in `tests/pass` under the address, leak and UB checks.

## M2 — `for` and iteration

```skuld
for i in 0..len(bytes) { ... }
for item in items { ... }
```

Small, and every later milestone consists of writing scanners and parsers.
It also defers the callback question honestly: `for` covers most of what
`numbers.sort((a, b) => a - b)` in `test.skuld` is reaching for, without fixing
a lambda syntax the user has explicitly marked for revision.

- **In scope:** half-open ranges `a..b`, iteration over `[]T` by value, the
  existing `break` and `continue`.
- **Out of scope:** a generic iterator protocol, lambdas, `map`/`filter`,
  `sort`.
- **Open risk:** the temptation to introduce an `Iterator` interface. With no
  interfaces and no generics, a `for` specialized to arrays and ranges in the
  checker is the honest construct; a generic protocol would be generics by
  accident.

## M3 — `Result<T, E>` and propagation

```skuld
func parse(text: string) -> Result<JsonValue, JsonError> {
    let n = parse_number(text)?
    ...
}
```

The error mechanism has to exist before anything that can fail exists.

- **Decision required first:** builtin `Result`, following the existing
  `Option` precedent, or general generics. Builtin avoids two-parameter
  generics and matches what is already there, at the cost of known debt against
  a future generic system. General generics is a milestone of its own and
  `AGENTS.md` defers it.
- **In scope:** `Result<T, E>`, `Ok`/`Err`, `if let Ok(x) =`, `match`, and `?`
  as an early return.
- **Out of scope:** automatic error conversion across types, backtraces,
  recoverable panics.
- **Open risk:** `?` interacts with reference counting — an early return must
  release everything acquired in the scope. The emitted cleanup attributes
  should already cover this, which is exactly why a leak here would go
  unnoticed. Needs a dedicated fixture under the leak checker.

## M4 — Bytes, sized integers and string slices

Being able to look inside a string and build one from bytes.

- **In scope:** `u8` and probably the rest of the sized integers at once, since
  the machinery is shared; `[]u8`; byte indexing into a string; slicing;
  `bytes_to_string()` returning a `Result` with UTF-8 validation; efficient
  string building, possibly just `[]u8` plus the growth operations.
- **Why after M3:** converting bytes to a string must fail on invalid UTF-8.
  Without `Result` the only option is a trap, which is wrong for network data.
- **Out of scope:** encodings other than UTF-8, a `char` or code point type,
  normalization, regular expressions.
- **Open risk:** strings are currently length-aware views of static literal
  bytes, with reference-counted concatenation. Slices pointing into a
  reference-counted string require deciding whether a slice retains its owner or
  copies. Retaining is faster and more dangerous, since a three-byte slice can
  hold a large buffer alive. Default to copying and revisit with a benchmark.
- **Closing marker:** a complete JSON parser written in **pure Skuld** over
  `[]u8`, returning `Result<JsonValue, JsonError>`, as a `tests/pass` fixture.
  That is the whole of `res.json()` except for where the bytes come from.

## M5 — `extern "C"` FFI and linking

```skuld
extern "C" {
    func read(fd: i32, buf: *u8, count: u64) -> i64
}
```

The boundary with the outside world, and the real gate for any network work.

- **In scope:** `extern` declarations, opaque pointer types, marshalling
  `string` and `[]u8` as borrowed pointer plus length, link flags in
  `cli/src/native.rs`, and some marker for the unsafe boundary.
- **Out of scope:** Skuld callbacks into C, passing structs by value across the
  ABI, varargs, non-C foreign interfaces.
- **Open risk:** the largest in the plan. The reference-counted memory model
  meets code that knows nothing about retain and release. Fix the rule early:
  managed values never cross the boundary; only scalars and borrowed
  pointer-plus-length whose lifetime ends with the call.
- **Validation:** `read_file()` over libc, not sockets. It exercises the FFI,
  `[]u8`, `Result` and slicing at once without TLS or networking in the way.

## Beyond M5

With M1–M5 in place, an HTTP request stops being a language problem and becomes
a library problem: sockets through the FFI, a response parser in Skuld, and the
JSON parser from M4. What remains undecided is where those functions live —
`import` is still only a reserved word rejected by the parser. Modules and a
minimal standard library therefore fall between M5 and any `fetch`, either
interleaved or as a milestone of their own.

## Open design questions

These are not settled by this document and change the shape of the milestones
above.

1. Builtin `Result` or general generics? Determines M3 and whether generics ever
   enter the project.
2. Do string slices retain their owner or copy? Determines the memory profile of
   all parsing.
3. Recursive enum variants: automatic boxing, or the user's responsibility?
4. Callback and lambda syntax, deferred by M2 but eventually demanded by
   sorting. The current sketch in `test.skuld` is marked unsatisfactory by the
   user.
5. JSON objects as a list of key/value fields, avoiding a hash map milestone
   entirely, or waiting for a real map type?
