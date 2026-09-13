# Skuld

Skuld is an experimental statically typed native programming language.

```skuld
func main() {
    print("Hello from Skuld!")
}
```

Skuld now checks and runs small native programs through the complete pipeline:
lexer → parser → AST → resolver → type checker → HIR → C → clang → executable.
Demos 0–3 work: hello, typed functions/variables, conditional flow and classes
with methods and interpolation. Loops, structs, weak references, arrays,
Option values, `Result<T, E>` with `?` propagation, the sized integer types and
byte-level string access work too.

Skuld is an independent language with its own syntax, semantics and identity.
TypeScript is only one reference for readability, alongside Go, V and Rust;
copying its syntax or creating a second TypeScript is explicitly not a goal.
Design decisions follow Skuld's needs, not resemblance to another language.
The existing `func`, `->`, immutable `let` and mutable `var` remain unchanged.

The user's [test.skuld](test.skuld) contains evolving syntax demonstrations.
Classes are implemented with methods such as `hello()`, implicit `this`,
reference semantics and `new User(...)` construction; initialization and
global-statement proposals are documented separately; sorting ships with M8.

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

`skuld --help` lists every command, option and exit code, and `skuld --version`
prints the version. Options may sit on either side of the file, and `--` ends
them, so a source whose name begins with a dash can still be named:

```bash
skuld run -lm program.skuld
skuld build program.skuld -o bin/program
skuld check -- -odd-name.skuld
```

`check` is silent on success. Compiler errors show source locations and return
exit code 1; invalid CLI usage returns 2. `run` checks first, generates C in a
private temporary directory, invokes clang, executes the binary and cleans up.
Program stdin/stdout/stderr are inherited and its exit status is propagated
(Unix signals map to 128 + signal). No persistent build artifacts are produced.
`build` performs the same checks and clang invocation but keeps the executable
instead of running it; a rejected program leaves no executable behind.
`fmt` formats a source file in place according to the official style,
preserving all comments and literal representations. `fmt --check` checks
whether a file conforms without modifying it, returning exit code 1 on drift.

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
`build` compiles to a native executable and keeps only that artifact: in the
working directory under the source file's stem, or wherever `-o <path>` names.
A build never overwrites its own source. `fmt` formats source files and `test`
runs the `test_...` functions in a file; `new` and `doc` remain future CLI
commands. `run` hands a program its own arguments after `--args`, which are
passed through unread — `--` keeps its separate meaning of ending the compiler's
own options.

```bash
cargo run -p skuld-cli -- run examples/jsontool.skuld --args document.json user.name
```

```bash
cargo run -p skuld-cli -- test examples/jsontool_tests.skuld
```

A test is a top-level `func test_...()` that takes nothing and returns nothing.
The file is compiled once with an entry point the runner writes, so the suite is
one program: the first failure stops it, and the report says which tests never
started. A failing suite exits non-zero.

A program that declares foreign functions from a library other than libc names
it on the command line; `build` and `run` forward `-l` and `-L` to clang and
accept no other linker argument, so nothing here can redirect the output or
change how the program itself is compiled:

```bash
cargo run -p skuld-cli -- run examples/ffi.skuld
skuld build program.skuld -L/opt/lib -lfoo
```

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
of C undefined behavior. String literals reference immutable static bytes;
Unicode and embedded NUL are preserved. Concatenation and classes allocate
and are reference counted. There is no string mutation or cycle collector. Homogeneous arrays and weak
class references are supported; strong cycles require weak links or explicit breaking.

`unsafe extern "C" { ... }` declares functions from a linked library, and
`ptr(value)` borrows the bytes of a string or an array to pass to one. Only
scalars and raw pointers cross that boundary: a reference-counted value never
does.

A lambda is written without a keyword, the way a method already declares
itself, and a function type keeps `->`:

```skuld
var numbers = [5, 3, 9, 1]
numbers.sort((a: int, b: int): int { return a - b })

func count_if(values: []int, keep: (int) -> bool) -> int { ... }
```

A function value may be a parameter or a local and nothing else — never a
field, a return type or a payload. With reference counting and no cycle
collector, a managed object able to reach a closure that captured it would be a
cycle nothing frees; the rule removes the reachability, and in exchange a
function value never allocates, retains or releases. Captures are copies of
immutable bindings. Storable class interfaces support handlers kept for later; closures remain non-escaping.

The library ships inside the compiler behind a reserved `std` prefix, and
reaches as far as fetching a document and decoding it:

```skuld
import "std/http"
import "std/json"

let response = http.get("http://127.0.0.1:8080/data.json")
```

Plain HTTP only, and to an IPv4 address: TLS is refused by name, and there is
no resolver, because reading through a pointer is out of scope at the foreign
boundary and every resolver in libc answers with one. Both say so where they
are used.

A program can span several modules. A module is a directory whose `.skuld`
files share one namespace; `import "net/socket"` binds the path's last segment,
so its exports are reached as `socket.connect(...)` and never unqualified.
`pub` marks what leaves a module, a method is public with the type that owns
it, and an import cycle is an error. Paths resolve against the directory of the
entry file and cannot climb out of it. See [LANGUAGE.md](LANGUAGE.md) for the
complete implemented/planned distinction.

```skuld
// geometry/point.skuld
pub struct Point {
    x: int
    y: int
}

// main.skuld
import "geometry"

func main() {
    print(geometry.Point { x: 3, y: 4 }.x)
}
```

`std` is a reserved import prefix: the standard library is written in Skuld and
embedded in the compiler binary, so it needs no installation and a directory
named `std` beside a program cannot replace it. It is deliberately small —
`std/utf8` decodes bytes with a real error type, `std/strings` has byte-offset
helpers, `std/cstring` builds the NUL-terminated buffer C expects, `std/fs`
reads and writes whole files, `std/os` reaches the process arguments, exit
status and `errno`, `std/map` is a string-keyed index, `std/dns` resolves host
names by speaking DNS over UDP, and `std/testing` holds the assertions
`skuld test` runs. `std/tls` and `std/https` are the exception to the
no-dependencies rule and are opt-in: a program that imports them links OpenSSL
itself with `-lssl -lcrypto`, and verification cannot be turned off.

```skuld
import "std/strings"

func main() {
    print(strings.trim("  HTTP/1.1 200 OK \r\n"))
}
```

## Architecture and verification

One Cargo workspace contains the `skuld-compiler` library, `skuld-cli` binary
package and `skuld-lsp` language server, without external Rust dependencies. Recursive descent and Pratt parsing
remain separate from resolution and checking. The type checker builds semantic
tables; a distinct lowering pass creates typed HIR. C generation reads only HIR.
External process invocation belongs to the CLI, never the compiler library, and
so does reading files: the module graph asks a loader the caller supplies.

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

The verified milestones include the complete native pipeline, Demos 0–3, loops,
structs, weak class references and arrays.
Rust builds the compiler, and clang compiles the emitted C. Programs execute
natively without JavaScript or a VM.

`while`, `loop`, `break` and `continue` execute. A `loop` with no `break`
diverges, so it satisfies a non-void return type.

`struct` declares a value type with fields, methods, record construction and
field assignment. Assignment copies value fields; class and array fields keep
their shared references. Methods take an implicit, immutable `this`.

Strings are reference counted: `+` concatenates and the result is freed when
its last reference goes away. Counts are not atomic, there is no garbage
collector and no cycle collector; literals never allocate.

`"${value}"` interpolates, accepting what `print` accepts and lowering to
concatenation.

`class` declares a reference type with fields, methods, `new` construction
and reference semantics: multiple bindings share mutable state. Allocation is
heap-based and reference counted.

`weak User` holds a non-owning class reference. `weak(user)` creates one,
`upgrade()` returns `Some(user)` or `None` and safely retains a live target.
`alive()` and trapping `get()` remain available. Weak parent
links avoid ownership cycles without introducing null or a cycle collector.

Arrays use `[]int` and `[1, 2, 3]`, with shared references, checked indexes and
`len()`. They support growth, insertion, removal, slicing and stable in-place sorting
with non-escaping callbacks.

```bash
cargo run -p skuld-cli -- run examples/classes.skuld
cargo run -p skuld-cli -- run examples/weak.skuld
cargo run -p skuld-cli -- run examples/arrays.skuld
cargo run -p skuld-cli -- run examples/options.skuld
cargo run -p skuld-cli -- run examples/enums.skuld
cargo run -p skuld-cli -- run examples/for_loops.skuld
cargo run -p skuld-cli -- run examples/results.skuld
cargo run -p skuld-cli -- run examples/bytes.skuld
cargo run -p skuld-cli -- run examples/ffi.skuld
```

`Option<T>`, `Some(value)` and `None` represent optional values; contextual
`null` is another spelling of absence, not a standalone null value.
Use `if let Some(value) = expression { ... } else { ... }` to access a payload;
`is_some()` and `is_none()` query presence. Options have value semantics and
an inline representation, with reference counting for managed payloads.

`enum` declares a sum type with unit and payload variants (`enum Status { Active, Inactive(int) }`).
`match` statements provide exhaustive pattern matching (`match val { Status.Active: ..., Status.Inactive(code): { ... }, _: ... }`)
with arm bindings and reference-counted managed variant payloads.

`for` loops iterate over half-open integer ranges `a..b` and arrays `[]T` by value
(`for i in 0..10 { ... }`, `for item in items { ... }`). Loop variables are immutable and
scoped to the body; managed array elements retain and release per iteration. `break` and `continue`
are supported.

`Result<T, E>` is the builtin error type. `Ok(value)` and `Err(error)` construct
one, and both take their type from the context, since neither side can be
inferred from the other. `match`, `if let Ok(value) = ...`, `if let Err(e) = ...`,
`is_ok()` and `is_err()` inspect it. The postfix `?` unwraps a success or returns
the error unchanged from a function that returns a `Result` with the same error
type, releasing everything the scope had acquired.

```skuld
func port() -> Result<int, ConfigError> {
    let text = lookup("port")?
    return Ok(to_int(text)? + 1)
}
```

The sized integers `i8 i16 i32 i64` and `u8 u16 u32 u64` join `int`, which is a
spelling of `i64`. A literal takes the width its context expects and is
range-checked there; widths never mix implicitly, and converting is an explicit
call that traps out of range (`int(byte)`, `u8(wide)`). Arithmetic traps on
overflow at every width.

Strings are byte sequences: `text.len()` counts bytes, `text[i]` reads a `u8`,
`text[a..b]` slices, and `text.bytes()` yields `[]u8`. Arrays slice the same
way. Slices copy, so a short view never keeps a large buffer alive.
`bytes_to_string(bytes)` validates strict UTF-8 and returns
`Result<string, string>`, which is how a `[]u8` built with `push` becomes a
string.

```skuld
func shout(text: string) -> Result<string, string> {
    var out: []u8 = []
    for byte in text.bytes() {
        if byte >= 97 && byte <= 122 { out.push(byte - 32) } else { out.push(byte) }
    }
    return Ok(bytes_to_string(out)?)
}
```

Foreign functions come from a linked library, declared in an `unsafe extern "C"`
block. Only scalars, raw pointers (`*u8`, `*void`) and a `void` return cross
that boundary; `ptr(value)` borrows the bytes of a string or an array for the
duration of a call and retains nothing.

```skuld
unsafe extern "C" {
    func write(fd: i32, buffer: *u8, count: u64) -> i64
}

func emit(text: string) -> int {
    return write(1, ptr(text), u64(text.len()))
}
```

This closes M7 (a minimal standard library), M6 (modules and `import`),
M5 (`extern "C"` FFI and linking), M4
(bytes, sized integers and string slices, closed by
`tests/pass/json_parser.skuld`), M3 (`Result<T, E>` and propagation), M2 (`for`
and iteration) and M1 (Enums and `match`), alongside
Option, classes, weak references and arrays.
M8 (function values and stable sorting), M9 (blocking TCP, HTTP and JSON)
and M10 (class interfaces) are also implemented, as is `let ... else`.
M11 (the official formatter, `skuld fmt`), M12 (LSP refactoring), M13 (local
CLI applications and `skuld test`), M14 (field defaults), M15 (a string-keyed
map), M16 (host names and system error reasons), M17 (verified HTTPS) and M18
(measured performance on two native targets) are implemented too. The LSP provides
diagnostics, completion, hover, definition, find-references, rename, the
document outline and formatting through `skuld fmt`'s own formatter, and
`examples/jsontool.skuld` is a multi-module native tool with its own test suite.

No implementation milestone is active, and the sequence [ROADMAP.md](ROADMAP.md)
proposed is finished. What comes next is a selection nobody has made; the
candidates listed there — general generics and self-hosting among them — each
need an explicit decision first.
Status is reported as completed milestones, not as a completion percentage, and
implies no production readiness. Performance is measured in
[BENCHMARKS.md](BENCHMARKS.md): between 1.2x and 3.8x the faster of Rust and Go
across five workloads on one machine, which is evidence for the range the
project aimed at and not a promised ratio.
