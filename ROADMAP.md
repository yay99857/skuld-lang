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

## M4 — Bytes, sized integers and string slices — Primitives implemented

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
- **Closing marker — outstanding:** a complete JSON parser written in **pure
  Skuld** over `[]u8`, returning `Result<JsonValue, JsonError>`, as a
  `tests/pass` fixture. That is the whole of `res.json()` except for where the
  bytes come from. The primitives above were delivered first, by explicit
  decision, so this marker still closes the milestone.

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

1. ~~Builtin `Result` or general generics?~~ Answered: M3 shipped `Result` as a
   builtin. Whether general generics ever enter the project, and how they would
   reconcile with the builtin `Option` and `Result`, remains open.
2. ~~Do string slices retain their owner or copy?~~ Answered: they copy, except
   for slices of string literals, whose bytes are static. Whether a retaining
   slice earns its danger is a question for a benchmark, not for this document.
3. Recursive enum variants: automatic boxing, or the user's responsibility?
4. Callback and lambda syntax, deferred by M2 but eventually demanded by
   sorting. The current sketch in `test.skuld` is marked unsatisfactory by the
   user.
5. JSON objects as a list of key/value fields, avoiding a hash map milestone
   entirely, or waiting for a real map type?
