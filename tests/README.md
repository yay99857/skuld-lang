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
`reserved_loops` became `pass/while_loop`, and `reserved_iteration` (`E1003`),
`reserved_members` (`E0110`) and `reserved_declaration` (`E1002`) now hold the
reserved-syntax coverage until their own milestones.

A promoted fixture must terminate. `loop` with no `break` never returns, so it
belongs in `fail/` or must carry an exit; never park an unbounded loop in
`pass/`, which would hang the suite instead of failing it.

`pass/` also pins the semantics `AGENTS.md` requires preserving —
left-to-right evaluation, boolean short-circuiting, compound assignment
snapshots, precedence and lexical shadowing. Changing any of those should
break a fixture.
