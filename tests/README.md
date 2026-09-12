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

`reserved_loops` (`E1003`) is deliberately a tripwire: when `while` and `loop`
land, that fixture stops failing and the suite goes red. That is the signal to
move it to `pass/` with a `.out`, not to delete it. `reserved_members`
(`E0110`) and `reserved_declaration` (`E1002`) hold the reserved-syntax
coverage until Demo 3, so it does not disappear when loops arrive.

`pass/` also pins the semantics `AGENTS.md` requires preserving —
left-to-right evaluation, boolean short-circuiting, compound assignment
snapshots, precedence and lexical shadowing. Changing any of those should
break a fixture.
