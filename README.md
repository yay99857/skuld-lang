# Skuld

Skuld is an experimental statically typed native programming language.

```skuld
func main() {
    print("Hello from Skuld!")
}
```

Skuld now checks and runs small native programs through the complete pipeline:
lexer → parser → AST → resolver → type checker → HIR → C → clang → executable.
Demos 0–2 work: hello, typed functions/variables and conditional control flow.

Skuld is an independent language with its own syntax, semantics and identity.
TypeScript is only one reference for readability, alongside Go, V and Rust;
copying its syntax or creating a second TypeScript is explicitly not a goal.
Design decisions follow Skuld's needs, not resemblance to another language.
The existing `func`, `->`, immutable `let` and mutable `var` remain unchanged.

The user's [test.skuld](test.skuld) contains evolving syntax demonstrations.
The [planned class design](LANGUAGE.md#classes-structs-and-memory--planned) now
uses methods such as `hello()`, implicit `this` and `new User(...)`. These are
specification proposals, not implemented compiler features; initialization,
global-statement and sorting questions are documented separately.

The priorities are simplicity, strong static typing, useful diagnostics and
native performance targeting the Go/Rust range. Performance is a goal to
validate with future benchmarks, not a current guarantee. Skuld does not aim
for TypeScript/JavaScript compatibility. See the [design priorities](LANGUAGE.md#project-priorities--design-commitments).

## Build and try

Install Rust stable with Cargo, rustfmt and Clippy (via rustup), and install
`clang` on PATH for native execution. `check` does not require clang.
The current native path is tested on Linux x86_64; other platforms are unverified.

```bash
cargo build
cargo run -p skuld-cli -- check examples/hello.skuld
cargo run -p skuld-cli -- run examples/hello.skuld
# Hello from Skuld!
cargo run -p skuld-cli -- run examples/functions.skuld
# 42
cargo run -p skuld-cli -- run examples/conditionals.skuld
# Adult
```

To install the CLI on your PATH:

```bash
cargo install --path cli
skuld check examples/functions.skuld
skuld run examples/functions.skuld
```

If an older `skuld` is already installed, run `cargo install --path cli --force`
to update it. Without installation, use `./target/debug/skuld` after `cargo build`.

`check` is silent on success. Compiler errors show source locations and return
exit code 1; invalid CLI usage returns 2. `run` checks first, generates C in a
private temporary directory, invokes clang, executes the binary and cleans up.
Program stdin/stdout/stderr are inherited and its exit status is propagated
(Unix signals map to 128 + signal). No persistent build artifacts are produced.

Debugging and inspecting generated code:

```bash
cargo run -p skuld-cli -- lex examples/hello.skuld
cargo run -p skuld-cli -- parse examples/functions.skuld
cargo run -p skuld-cli -- resolve examples/functions.skuld
cargo run -p skuld-cli -- emit-c examples/functions.skuld > generated.c
clang -std=c11 -O2 generated.c -o generated-program
./generated-program
```

`lex`, `parse` and `resolve` inspect their respective stages; they do not perform
a full type check. `emit-c` does run every compiler stage through C generation.
The debug AST and resolution output are not stable serialization formats.
`build`, `new`, `fmt`, `test` and `doc` remain future CLI commands.

## Implemented language core

```skuld
func add(a: int, b: int) -> int {
    return a + b
}

func main() {
    var age = 17
    age += 10
    let answer: int = add(20, 22)

    if age >= 18 {
        print(answer)
    } else {
        print("Minor")
    }
}
```

Implemented types: `int` (i64), `float` (f64), `bool`, `string` and `void`.
Parameters are immutable. Calls and operations require matching types; no
implicit numeric conversions or truthiness. Non-void functions must return on
all paths. `print()` emits a blank line; `print(value)` accepts one int, float, bool or
string and appends a newline. The previous `fn`, `function` and `println` spellings are not aliases.

Integer overflow and division/remainder by zero produce runtime errors instead
of C undefined behavior. Strings currently reference immutable static literal
bytes; Unicode and embedded NUL are preserved. There is no string allocation,
concatenation, interpolation, array support, class support or ARC yet. See
[LANGUAGE.md](LANGUAGE.md) for the complete implemented/planned distinction.

## Architecture and verification

One Cargo workspace contains the `skuld-compiler` library and `skuld-cli` binary
package, without external Rust dependencies. Recursive descent and Pratt parsing
remain separate from resolution and checking. The type checker builds semantic
tables; a distinct lowering pass creates typed HIR. C generation reads only HIR.
External process invocation belongs to the CLI, never the compiler library.

```bash
cargo fmt --all -- --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Full workspace tests require clang, including native integration tests and a
standalone generated-C check with UndefinedBehaviorSanitizer. Compiler-only
unit tests can run with `cargo test -p skuld-compiler` without clang.
Language fixtures are documented in [tests/README.md](tests/README.md).

## Progress and next milestone

The verified milestone is the complete native pipeline, Demos 0–2 and loops.
Rust builds the compiler, and clang compiles the emitted C. Programs execute
natively without JavaScript or a VM.

`while`, `loop`, `break` and `continue` execute. A `loop` with no `break`
diverges, so it satisfies a non-void return type.

Next: the planned class/object/method/interpolation showcase (Demo 3). Structs, managed memory, modules, standard library, official
formatter, broader tooling, portability and eventual self-hosting remain ahead.
Status is reported as completed milestones, not as a completion percentage, and
implies neither production readiness nor measured Go/Rust performance.
