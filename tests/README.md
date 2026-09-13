# Language tests

Golden fixtures for the whole pipeline. The runner is `cli/tests/golden.rs`;
compiler unit tests stay beside their modules and CLI behavior tests in
`cli/tests/cli.rs`.

| Directory | Command | Contract |
| --- | --- | --- |
| `pass/` | `skuld run` | Exits successfully; stdout matches `<name>.out` byte for byte |
| `fail/` | `skuld check` | Exits with failure, emits no stdout, and reports every diagnostic code in `<name>.err` |
| `trap/` | `skuld check` then `skuld run` | Passes static checking, then aborts at runtime with the message in `<name>.err` |

Add a fixture by dropping the `.skuld` source and its `.out`/`.err` expectation
into the matching directory; the runner discovers them automatically.

`pass/` and `trap/` need `clang` on PATH and are skipped without it. `fail/`
runs anywhere, since static checking never invokes external tools.

Fixtures cover only implemented behavior. Reserved syntax belongs in `fail/`
with its diagnostic code until its milestone arrives.

Reserved-syntax fixtures are deliberate tripwires: when a milestone lands, the
fixture stops failing and the suite goes red. That is the signal to promote it
to `pass/` with a `.out`, not to delete it. Loops went through exactly that:
`reserved_loops` became `pass/while_loop`, and classes similarly added `pass/class_reference_semantics`
while `reserved_declaration` (`E1002`, covering `interface`), `reserved_iteration` (`E1003`)
and `reserved_members` (`E0110`) hold reserved-syntax coverage until their own milestones.

A promoted fixture must terminate. `loop` with no `break` never returns, so it
belongs in `fail/` or must carry an exit; never park an unbounded loop in
`pass/`, which would hang the suite instead of failing it.

`pass/` also pins the semantics `AGENTS.md` requires preserving —
left-to-right evaluation, boolean short-circuiting, compound assignment
snapshots, precedence and lexical shadowing. Changing any of those should
break a fixture.

Classes, weak references and arrays have native fixtures for shared mutations,
managed fields/elements, weak expiration and cycle breaking. Bounds and expired
promotion failures live in `trap/`; type errors live in `fail/`. Every `pass/`
fixture also runs with address, leak and undefined-behavior sanitizers through
`cli/tests/native.rs`, where clang is required.

Option fixtures cover expected-type inference, managed and nested payloads,
constructor shadowing, if-let scope/flow and safe weak promotion on live, empty
and expired targets. Invalid Option recursion through value fields is rejected.

Result fixtures cover Ok/Err construction under an expected type, exhaustive
matching over the two variants, `if let Ok`/`if let Err`, `is_ok`/`is_err`, and
`?` propagation. `result_propagation` deliberately fails on both the first and
the second `?`, the latter with an array, a class and a string alive, so the
leak checker sees an early return that has to release them. Missing context for
a constructor, a mismatched or absent enclosing error type, a non-Result operand
of `?`, a non-exhaustive match and a redeclared `Result` live in `fail/`.

Sized integer fixtures cover every width's boundaries, trapping arithmetic,
explicit conversions in both directions and the refusals that keep widths from
mixing. Byte fixtures cover `len()`, indexing, slicing of strings and arrays,
`bytes()` and `bytes_to_string()`, including the UTF-8 sequences a lax
validator would wave through: overlong encodings, surrogate halves, truncated
sequences and code points above U+10FFFF. `string_bytes_and_slices` slices both
literal and owned strings, and arrays of strings and classes, so the leak
checker sees the copying path with managed elements. Out-of-range conversions,
out-of-bounds indexes and reversed slices live in `trap/`.

`json_parser` is M4's closing marker: a complete JSON parser written in pure
Skuld over `[]u8`, with no help from the compiler. It exercises the whole
milestone at once — byte indexing, slicing, sized integers, `bytes_to_string`,
arrays, enums, `match`, `Option`, `Result` and `?` — and it carries its own
failure cases, so both the accepted and the rejected documents are pinned by
the same `.out`. Its numbers are printed through `%.17g`, so a value with no
exact binary form shows the digits the double actually holds; that is the
language's float rendering, not the parser's. Being a `pass/` fixture, it also
runs under the address, leak and UB sanitizers, which is what makes it a
memory-management test of recursive managed values rather than only a parsing
test.

Foreign-boundary fixtures cover the `extern "C"` path: `extern_c_ffi` calls libc
`write` and `abs`, borrowing a string and a `[]u8` through `ptr`, and writes
every line through `write` so the expectation does not depend on stdio
buffering. Under the sanitizers it is also the proof that a borrow retains and
leaks nothing. The refusals that keep the boundary narrow live in `fail/`: a
managed type in a signature, a pointer to one, `ptr` on a value that owns no
bytes, a missing `unsafe` marker and a body on a declaration.
