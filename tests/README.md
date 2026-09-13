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
