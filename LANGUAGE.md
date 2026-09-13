# Skuld language specification

Status labels: **Implemented** means available now; **Planned** describes future
intent, not accepted/executable programs; **Experimental** denotes provisional
choices. The complete single-file native pipeline, Demos 0–2, loops, structs,
reference-counted strings, interpolation, classes, weak references, arrays,
builtin Option values and builtin `Result<T, E>` with `?` propagation are
implemented. Self-hosting remains planned.

## Philosophy — Planned

Static, strong typing with local inference and explicit API signatures. Skuld
has its own syntax, semantics and identity. TypeScript is only one reference
for readability, alongside Go, V and Rust. Skuld must not become a syntax copy,
a TypeScript dialect or a second TypeScript. Resemblance is not an acceptance
criterion for language features; simplicity, predictability, diagnostics and
Skuld's native execution model guide design decisions.
The existing `func`, `->`, immutable `let` and mutable `var` syntax is retained.
TypeScript/JavaScript source, type-system and runtime compatibility are not goals.
Native compilation initially through C and clang. Composition instead of
inheritance. No normal `null` value and no exceptions as the primary error model.
An official `skuld fmt` will eventually be the authority on style.

## Project priorities — Design commitments

1. Keep the existing syntax and semantics: `func`, `->`, `let` immutable, `var`
   mutable, explicit function signatures and local type inference. Preserve
   Skuld's independent identity: TypeScript is an optional readability reference,
   not a syntax specification or a roadmap. Adopt ideas only when they serve
   Skuld's own goals; do not copy features to match TypeScript or import
   JavaScript coercions, dynamic typing or runtime behavior.
2. Target native performance in the Go/Rust range for comparable workloads.
   This is a goal, not a measured guarantee. Future benchmarks must validate
   generated code, allocation costs and memory behavior before performance
   claims; a C backend alone does not guarantee this result.
3. Prefer simplicity, predictability and actionable diagnostics. Keep concrete
   numeric types, value semantics for structs and reference semantics for
   classes; reference counting must make allocation and reference-management costs
   understandable. Do not introduce a GC or borrow checker.
4. Build small verified milestones in order: lexer → AST/parser → resolver →
   type checker → HIR → C backend → native Demo 0, then Demos 1–3. Keep stages
   separate, preserve spans, avoid premature abstractions and dependencies,
   and do not implement classes before the function/type foundation is stable.
5. Treat this document as the source of design priorities. Documentation of a
   future feature does not authorize implementing it in an earlier milestone.

## Lexical grammar — Implemented

Sources are UTF-8 files with extension `.skuld`. Identifiers are ASCII:
`[A-Za-z_][A-Za-z0-9_]*`. Whitespace is ASCII whitespace and is discarded; `//`
comments extend to CR, LF or EOF. Newlines do not produce tokens. There is no
semicolon token or automatic semicolon insertion.

Function declarations use `func`, replacing the earlier `fn` and `function` spellings.
The builtin is `print`, replacing `println`; neither old spelling is an alias.
The old words are ordinary identifiers and can be explicitly declared by users.

Keywords: `func let var return if else while loop for in break continue new weak class struct
impl enum match`.
Reserved future keywords: `interface import static extern`.
`true` and `false` produce boolean literal tokens. Type names, `print`, `Some`,
`None`, `Ok` and `Err` are identifiers. `Option<T>` and `Result<T, E>` are
builtin type syntax, not user-defined generics. In a type annotation,
`Option<int>=None` and `Result<int, string>=value` separate the closing `>`
from assignment even though the lexer otherwise recognizes `>=` as one operator.
`>>` is two closing tokens, never a shift, so nested generic types need no
special rule. Recognizing a keyword does not implement its syntax or semantics.

Delimiters: `( ) { } [ ] , . .. : ->`.
Operators: `+ - * / % = == != < > <= >= ! ? && || += -= *= /=`.
`?` is postfix and only valid after an expression; see error handling below.
Operators use longest matching; a sign is separate from a number.

Integers are `[0-9]+`, stored as `u64` magnitudes. Floats are
`[0-9]+ '.' [0-9]+`, stored as finite `f64`. Leading zeros are decimal.
Integer magnitudes above `u64::MAX` and non-finite float results are lexical
errors. Signed `int` range checking is deferred to semantic analysis, so the
magnitude in `-9223372036854775808` remains representable. Decimal float parsing
uses normal f64 rounding. A dot without a digit on both sides is a separate
token, e.g. `1.foo`, `.5`, and `1.`. Exponents, bases, separators and numeric
suffixes are not supported; they can lex as adjacent tokens, which the parser rejects in a numeric expression.

Strings use double quotes; chars use single quotes and must decode to exactly
one Unicode scalar value (not one grapheme). Both accept Unicode text and the
escapes `\\`, `\"`, `\'`, `\n`, `\r`, `\t`, `\0`, `\$`. Literal line breaks
are rejected. `${expr}` interpolates; `\$` writes a literal `${`, and a `$`
not followed by `{` is ordinary text.
Block comments are unsupported; their delimiters currently lex as operators.

Tokens and diagnostics retain half-open byte spans. EOF occurs exactly once at
`[source.len(), source.len())`. Lexical errors accumulate; invalid characters
are skipped, bad quoted literals recover at their closing quote or line end,
and invalid numbers are consumed as a unit. Invalid literals emit no token.
The CLI suppresses tokens when any diagnostic exists.

Diagnostics use stable lexical codes: E0001 invalid character, E0002 invalid
number, E0003 unterminated literal, E0004 invalid escape, E0005 invalid char.
Locations are one-based lines and Unicode scalar columns. LF indexes lines;
CRLF displays without its CR. Tabs display as four spaces; terminal grapheme
width alignment and standalone-CR line indexing are future improvements.

## Types, variables and mutability — Implemented core

Initial semantic types: `int = i64`, `float = f64`, `bool`, `string`, `void`.
These aliases are platform independent. Later: `i8 i16 i32 i64`,
`u8 u16 u32 u64`, `f32 f64`, `uint`, and `char`.
`uint`'s alias is not yet specified. Semantic types use an enum, never source
spellings; the wider numeric names and `char` are not accepted semantic types yet.

```skuld
let age = 27
let explicit_age: int = 27
var count = 0
count += 1
```

`let` is immutable; `var` allows assignment. No implicit float-to-int conversion.
Child scopes allow shadowing; duplicate declarations in one scope are errors.
Unknown names, incompatible types and immutable assignment produce diagnostics.
Function parameters are immutable; copy a parameter into a local `var` to
modify it. `void` is valid only as a return type, not for locals or parameters.
Local variables always require an initializer and cannot store void results.

## Functions, expressions and control flow — Implemented core

```skuld
func add(a: int, b: int) -> int {
    return a + b
}
func main() {
    print(add(20, 22))
}
```

Parameters and return signatures are explicit; omitted return type means void.
The entrypoint is `func main()`. Executable global statements are forbidden.
`print` is a special builtin returning `void`. `print()` emits a blank line;
`print(value)` accepts one `int`, `float`, `bool` or `string` and appends a newline.
More than one argument is rejected. It is not a full standard library. `check` and
`run` require `main` with zero parameters and a void return (implicit or explicit).
Non-void functions must return on every path; both branches of an if must
return unless a later statement guarantees a return. Even unreachable source
is type-checked. A void function may use bare `return` or `return void_call()`.

Precedence, weakest to strongest: assignment; logical or; logical and;
equality; comparison; addition/subtraction; multiplication/division/modulo;
unary; call/member/index; primary. Calls and members include `foo(1, 2)`, `user.name`
and `user.greet()`. The parser preserves spans and remains separate from resolution and type
checking. Assignment is right-associative; other binary operators are
left-associative. Unary `+`, `-`, `!` bind below calls and members. Parentheses
produce explicit group nodes. Assignment targets are identifiers, member accesses or array indexes. Struct
field writes require a mutable place; writes through class or array references
can use an immutable binding. Assignments preserve the target type and yield the assigned value.
Comparison chains are parsed left-associatively and then type-checked normally.
Direct function/builtin calls, optionally grouped, and type-checked method calls
are supported. Callable values and indirect calls are rejected by the checker. Trailing commas are accepted in parameter and
argument lists. Local declarations require an initializer; annotations are
optional, but function parameters require types. AST type references preserve
source names, including unknown names; these are not semantic type values.

Blocks, return, expression statements, variables, `if`/`else` (including
`else if` and `if let Some(name) = value`), `while`, `loop`, `break` and `continue` are parsed. Later:
`for item in items`, `0..10` and `0..=10`. Arrays `[1, 2, 3]` with type
syntax `[]int` are implemented below.

### Loops — Implemented

`while condition { }` requires a `bool` condition, with no truthiness and no
parentheses around it. The body is a child scope like any other block, and the
condition is re-evaluated before every iteration, including after `continue`.

`loop { }` repeats until a `break` leaves it. `break` and `continue` bind to
the innermost enclosing loop; outside any loop they are `E0111`.

`for variable in start..end { }` iterates over the half-open range `[start, end)` of integers.
`start` and `end` are evaluated once before iteration begins; if `start >= end`, the body runs 0 times.

`for variable in array { }` iterates over each element of an array by value. If the array holds managed
values (strings, arrays, classes), each element is safely retained on entry to the iteration and
released on iteration exit (including upon `break`, `continue` or `return`).

The loop variable is an immutable binding scoped strictly to the loop body. Like `while`, `for` loops
may execute 0 times, so they do not satisfy a non-void return type. `break` and `continue` inside `for`
bind to the innermost enclosing loop.

A `loop` that no `break` can leave never falls through, so it satisfies a
non-void return type and any code after it is unreachable. Adding a `break`
restores the fall-through path and the return requirement returns with it. A
`while` or `for` never satisfies a return type, because its condition or collection may be empty on
entry.

## Statement boundaries and parser API — Implemented

The parser reads line breaks from source gaps between byte spans; the lexer
still emits no newline tokens. Expressions greedily continue through operators,
call parentheses, member dots and index brackets, including across newlines and comments:

```skuld
let result =
    foo()
    + bar()
```

Once an expression cannot continue, a newline or `}` terminates a variable,
return or expression statement. Adjacent simple statements on the same line
are rejected. Block statements and `if` statements are self-delimiting.
`foo` followed on the next line by `(bar)` is a call; `a` followed by `-b` is
subtraction. Start a separate statement with syntax that cannot continue the
previous expression. A bare `return` ends at a newline or `}`. For multiline
return values, begin the expression on the same line, e.g. `return (` followed
by the value and `)` on later lines. No semicolons are accepted.

`skuld_compiler::parse(&str) -> ParseOutput` coordinates lexing and the manual
parser; it returns an optional `Program` and diagnostics. Lexical errors stop
parsing. Syntax errors recover at statement/declaration boundaries; no partial
AST is exposed if any error occurs. Top-level function, struct and class
declarations are accepted. Empty files parse successfully but fail full checking without main.
Duplicate names and unknown value names are checked by the resolver; types,
entrypoints, return completeness and mutability are checked by the type checker.
`let age: int = "hello"` is syntactically valid but fails checking with E0102.

`skuld parse file.skuld` prints the debug AST, not a stable serialization format.
It is a temporary inspection command, not semantic `check`. Codes E1001–E1005
represent expected syntax, expected top-level declaration, unsupported syntax,
invalid assignment target and syntax resource limits. Parser nesting is bounded
at 64 recursive levels and expressions at fewer than 256 consumed tokens to
report pathological input instead of overflowing the compiler's stack. These
are implementation limits, not permanent language limits.

## Lexical name resolution — Implemented

`resolve(&Program) -> ResolveOutput` visits the unchanged parser AST and returns
resolution tables only when no diagnostics occur. `skuld resolve file.skuld`
runs lexer → parser → resolver, then prints debug tables (not a stable format).
It is an inspection command, not full semantic `check`.

Functions are declared before resolving bodies, allowing forward references and
mutual recursion. Value names share one namespace. Parameters and the outer
function body share a scope; nested blocks and each branch have child scopes.
Same-scope duplicate functions, parameters or locals produce E0202; the first
binding is retained during error recovery. Child scopes may shadow names.

Local bindings become visible after their initializer is resolved. Therefore
`let x = x` requires an existing outer binding; uses before a local declaration
are errors when no outer binding exists. Locals do not leak across blocks,
branches or functions. Unknown identifiers produce E0201 at the name span.

A separate parent prelude contains the typed builtin identities `Print`, `Some`
and `None`. User functions and local bindings may shadow `print`, `Some` or
`None`; checking and lowering use the resolved symbol, not its spelling. The
`Some(name)` pattern in `if let` is dedicated syntax, not a value-name lookup.
Its immutable binding exists only in the successful branch, shares that body
scope and is not visible in the initializer or `else` branch. The type checker
validates the builtin signature and the backend implements printing.

Resolution stores deterministic SymbolId/ScopeId tables, declaration spans,
parent scopes and local mutability metadata. Declaration/use maps are keyed by
name byte starts in the single original source revision; do not reuse them
after editing or transforming the AST. Type names remain syntax references for
the type checker. Member access resolves its receiver only; field and
method names await type-directed resolution. Immutable assignment, callable
validation, argument counts, return types and entrypoints are not checked here.
Parameters are recorded distinctly and enforced as immutable by the type checker.

## Type checking and native execution — Implemented

`check(&str)` runs parsing, resolution and type checking, returning an opaque
`TypedProgram` only on success. It owns the original AST and semantic tables;
no partial typed output escapes after errors. `lowering::lower` consumes it to
produce HIR with resolved symbol IDs, explicit types and source spans.
`codegen_c::emit_c` accepts only that HIR. `compile_to_c(&str)` composes these
stages without launching tools. The CLI alone invokes clang and the executable.

Numeric operators require two operands of the same numeric type. `+ - * /`
work on int and float; `%` only on int. Numeric ordering returns bool. Equality
and inequality work on matching int, float, bool or string values. Strings
compare byte content, not pointer identity. `&&`, `||` and `!` require bool;
there is no truthiness or implicit int/float conversion. Unary `+` and `-`
require numbers. Function values, chars, invalid member accesses and unknown types produce
explicit diagnostics rather than reaching code generation.

Evaluation is left to right, including call arguments. `&&` and `||`
short-circuit. Assignment evaluates its RHS and returns the new value;
compound assignment snapshots the target's old value before evaluating its RHS.
Class receivers and array indexes are evaluated once before the RHS, and the
containing allocation stays alive even if the RHS replaces another reference.
Construction evaluates field initializers in source order, independently of
the declaration order used for field layout.
Generated temporaries preserve these rules despite C's unspecified operand
and argument evaluation order.

`int` is signed 64-bit. Positive literal magnitudes must fit i64 except the
magnitude under unary minus in `-9223372036854775808` (grouping is allowed).
Integer addition, subtraction, multiplication and negation trap on overflow.
Division truncates toward zero; remainder has the dividend's sign. Division
and remainder by zero trap. MIN / -1 traps; MIN % -1 is zero. Runtime failures
print a message with a source byte offset to stderr and exit 1. Static checking
does not attempt constant arithmetic evaluation, so `1 / 0` passes checking
and fails only when evaluated. Runtime source-line rendering is future work.

Floats use binary64 operations without fast-math, with normal IEEE infinities
and NaNs possible during arithmetic; literal parsing still rejects non-finite
literals. Float printing uses 17 significant digits. Bool prints `true` or
`false`; int prints decimal. Every print appends a newline.

A string is a pointer, a length and an owner. Literals keep the owner null and
point at static bytes that live for the program duration; concatenation
allocates and reference counts the result. Printing and equality preserve
embedded NUL bytes. There is no string mutation yet.

Printing, comparison and checked-arithmetic helpers are emitted with the C;
integer checks use clang overflow builtins. Retain and release come from
`runtime/strings.c`, embedded verbatim rather than restated by the code
generator.

`skuld check file.skuld` succeeds silently with code 0 or renders diagnostics
with code 1, and never invokes clang. `skuld emit-c file.skuld` emits C after all
checks. `skuld run file.skuld` creates an exclusive temporary build directory
(private on Unix), writes C, invokes `clang -std=c11 -O2 -fno-fast-math`, executes
the binary with inherited stdin/stdout/stderr and cleans the directory on normal
completion or error. Child exit codes propagate; Unix signals map to 128 +
signal. A killed CLI may leave temporary files. Missing clang and tool/process
failures produce friendly CLI errors.

`skuld build file.skuld` runs the same checks and clang invocation, but writes
the executable to the working directory under the source file stem (with `.exe`
on Windows) and keeps it; only the generated C stays in the temporary
directory. A source path without an extension has a stem equal to the file
itself, so building it is refused rather than overwriting the source.

Compiler diagnostics added in this milestone: E0101 unknown/unsupported type,
E0102 type mismatch, E0103 invalid value type, E0104 invalid operator,
E0105 integer range, E0106 argument count, E0107 non-callable value,
E0108 missing return, E0109 invalid entrypoint, E0110 unsupported feature,
E0203 immutable assignment and E0204 invalid assignment destination.

## Structs — Implemented

```skuld
struct Vec2 {
    x: float
    y: float
}

func main() {
    var point = Vec2 { x: 10.0, y: 20.0 }
    point.x = 1.0
    print(point.y)
}
```

Fields are declared one per line, following the statement-boundary rule.
Record construction names every field exactly once: there are no defaults and
no partial initialization, so a missing field is `E0112` and a repeated one is
a duplicate declaration. Field order in construction is free; the backend lays
fields out in declaration order.

A struct cannot contain itself. A value type has no indirection, so the size
would not exist; the checker rejects it rather than the C compiler.

Values copy on assignment, argument passing and return. Assigning to `v.x` or
`v.i.x` through value fields requires the binding it is rooted in to be a `var`;
those fields of a `let` or parameter are immutable. Class and array fields
share references on copy, and mutation through those references is allowed.

Struct names live in a type namespace. The value resolver never sees them,
which is why resolution still reports only value names.

`if value { }` and `while value { }` read as a condition followed by a block,
never as record construction. Parentheses make a literal available again, as
does any nested expression context such as a call argument or a field value.

### Methods

```skuld
struct Rectangle {
    width: int
    height: int

    area() -> int {
        return this.width * this.height
    }
}
```

Methods are declared in the type body without `func` and without an explicit
receiver, matching the class syntax. `this` is the current value.

`this` is bound as an ordinary parameter, so it is immutable: a method cannot
assign to `this.field`, and since structs copy, the receiver is a copy and a
method never modifies its caller's value fields. Reference-valued fields retain
their sharing rules: a method can mutate an object or array referenced by a
struct field. Mutating value receivers remain future work.

A method name is not in ordinary scope. A sibling method is reached through
`this`, and a plain function cannot see methods at all. A method is not a
value either: `r.area` is an error and `r.area()` is the call.

Lowering turns a method into a function with the receiver as a leading
argument, so methods cost no more than a call.

Classes are implemented below.

## Memory — Implemented for strings, classes, arrays, options and weak references

Skuld manages memory with **reference counting, not a garbage collector**. Owning slots release their values when their lexical block exits. The last
strong reference releases the payload; class allocation storage can remain
until its last weak reference is also released.

Counts are **not atomic**: the language has no threads, so paying for atomic
operations on every copy would cost without buying anything. Introducing
threads means revisiting the runtime, not the code generator.

There is **no cycle collector**, by design. Strings are immutable and cannot
form cycles. Class and array graphs can form strong cycles, which must be
broken explicitly or designed with weak class links. Weak references do not
automatically detect or collect cycles.

String literals keep pointing at static bytes and never allocate. Only values
built at run time, such as concatenation results, are heap allocated.

```skuld
func greeting(name: string) -> string {
    return "Hello, " + name + "!"
}
```

`+` concatenates strings and produces a new one; no other operator is defined
on them, and there is no implicit conversion, so `"a" + 1` is a type error.

```skuld
print("${person.name} is ${person.age}, adult: ${person.age >= 18}")
```

Interpolation accepts exactly what `print` accepts — int, float, bool and
string — so a value reads the same interpolated or printed; anything else,
including a struct, has no textual form and is rejected. Booleans interpolate
to static bytes and do not allocate. Expressions may contain braces: a record
literal or a method call inside `${ }` does not end it early.

Interpolation lowers to concatenation, so it costs what writing the
concatenation by hand would.

Generated code releases every owning slot on every exit path, including
`return`, `break` and `continue`. Structs that hold strings are managed too:
copying one retains its fields and dropping one releases them. See
[runtime/README.md](runtime/README.md) for the emitted ownership rules.

Classes and arrays reuse this runtime with reference semantics. Weak class
references allow back-links without keeping the target alive. Compiler-created
owning temporaries currently live until the containing block exits, so weak
expiration can occur later than the last source-level use.

## Classes — Implemented

The user's [test.skuld](test.skuld) provided the direction for class syntax.
Classes are managed reference types with heap allocation and reference counting.
Unlike structs, two bindings to a class share the same underlying object.

```skuld
class User {
    name: string
    greeting: string

    hello() {
        print("${this.greeting}, my name is ${this.name}")
    }

    rename(to: string) -> User {
        this.name = to
        return this
    }
}

func main() {
    let user = new User(name: "Ada", greeting: "Hello")
    let alias = user
    alias.rename("Grace")
    user.hello()
}
```

Class methods use `hello()` or `is_adult() -> bool` without `func` and
without an explicit receiver parameter. `this` denotes the implicit current
instance inside instance methods. Top-level functions retain `func`.
Fields keep explicit type annotations, and method parameters and non-void
returns retain explicit types.

Class construction uses `new ClassName(field: value, ...)`. Every field must be
initialized in construction; there are no uninitialized reads, field defaults
or JavaScript-style `undefined` values. Constructing a class with `User { ... }`
or a struct with `new Struct(...)` is rejected.

Reference semantics and memory rules:
- An immutable `let` binding holds a constant reference to a mutable object: assigning
  to `user.name` or `this.name` is permitted. Rebinding `user` itself or assigning
  directly to `this` is governed by binding mutability and rejected for immutable bindings.
- Class fields can refer to the enclosing class or other classes without size cycles,
  because references are pointers.
- Classes allocate on the heap and begin with reference count 1. Passing, copying and
  returning adjust reference counts via retain and release. When count reaches zero,
  managed fields are released. Allocation storage is freed once no weak references remain.
- Weak references to break reference cycles are implemented below.

## Weak class references — Implemented

`weak User` is a distinct, statically typed non-owning reference. `weak(user)`
creates one from a class reference. Empty `weak()` needs an expected weak type
from an annotation, assignment, field, parameter, array element or return type.
There is no implicit strong/weak conversion and no normal null value.

```skuld
class User { name: string }

func main() {
    var observer: weak User = weak()
    {
        let user = new User(name: "Ada")
        observer = weak(user)
        if let Some(retained) = observer.upgrade() {
            print(retained.name)
        }
    }
    print(observer.alive()) // false
}
```

`upgrade() -> Option<User>` checks liveness and retains the target in one runtime
operation. A live target produces `Some(user)` owning a strong reference; an
empty or expired weak reference produces `None` without trapping. Handle the
result with `if let Some(user) = observer.upgrade() { ... } else { ... }`.

`alive() -> bool` remains a liveness query. The existing `get() -> User` retains
the target or traps with `expired weak reference` and a source byte offset.
Prefer `upgrade()` when expiration is an expected outcome: it combines the
check and promotion. All three methods take no arguments.

Weak values can be copied, assigned, passed, returned and stored in structs,
classes and arrays. They keep allocation bookkeeping alive, not the target's
managed fields. Only classes support weak references in this milestone.
Use a weak parent link with a strong child link to avoid ownership cycles; see
[examples/weak.skuld](examples/weak.skuld).

## Optional values — Implemented

`Option<T>` is a builtin value type representing a present value or `null` (also `None`).
`T` can be any non-void implemented value type, including another Option,
a struct, a class, an array or a weak reference. This milestone introduces
neither general generics nor user-defined enums.

```skuld
func answer(found: bool): Option<int> {
    if found { return 42 }
    return null
}

func main() {
    let missing: Option<int> = null
    if let value = answer(true) {
        print(value)
    } else {
        print("No answer")
    }
    print(missing.is_none())
}
```

Values wrap implicitly into an expected `Option<T>` (e.g. `return 42` or
`let x: Option<int> = 42`), while `Some(value)` remains supported.
`null` (and `None`) needs an expected Option type from an annotation, assignment, parameter,
field, return type or enclosing array/Some expression. For example,
`let nested: Option<Option<int>> = Some(null)` is valid, while unannotated
`let nested = null` cannot infer the payload. No implicit unwrapping,
truthiness or numeric conversions are provided. `null()` is invalid.
`Option` cannot be redeclared as a struct/class type; `null`, `Some` and `None` remain
shadowable prelude value bindings like `print`.

`if let name = expression { ... } else { ... }` (and `if let Some(name)`) evaluates the expression
once. The successful branch receives an immutable copy of the payload, retaining
any owned references. That name exists only in that branch. The `else` branch
is optional and can contain another `if`/`if let`; both branches must guarantee
a return for the statement to satisfy a non-void function's return requirement.
General patterns and `match` remain planned. There is no direct payload field or unchecked Option extraction API.

`is_some(): bool` and `is_none(): bool` query the tag and take no arguments.
Use `if let` to obtain the payload. Options do not support equality, printing or
interpolation as a whole; extract and use their payload instead.

Options store a tag and an inline payload; constructing an Option itself does
not allocate. Copying a Some copies value payloads and retains managed payloads.
None never reads, retains or releases its inactive payload. Thus a value struct
cannot contain itself through Option; class or array references break that
size cycle. Owning temporaries retain the existing lexical-block lifetime.

The existing record-construction ambiguity applies to bare `None` followed by
a standalone block: write `let value: Option<int> = (None)` before a following
`{ ... }`, so the parser does not read `None { ... }` as record construction.
See [examples/options.skuld](examples/options.skuld) for optional values and safe
weak promotion together.

## Error handling — Implemented

`Result<T, E>` is a builtin value type holding either a success payload of type
`T` or an error payload of type `E`. Both must be non-void implemented value
types. Like `Option`, it is builtin type syntax rather than user-defined
generics; general generics remain planned.

```skuld
enum ConfigError {
    Missing(string)
    NotANumber(string)
}

func lookup(key: string) -> Result<string, ConfigError> {
    if key == "port" { return Ok("8080") }
    return Err(ConfigError.Missing(key))
}

func port() -> Result<int, ConfigError> {
    let text = lookup("port")?
    return Ok(to_int(text)? + 1)
}
```

`Ok(value)` and `Err(error)` construct a `Result`. Both require an expected
`Result` type from an annotation, assignment, parameter, field, return type or
enclosing expression: writing one side says nothing about the other, so an
error type is never inferred from a success value. There is no implicit
wrapping of a bare value into a `Result`; the constructor is always written.
`Result` cannot be redeclared as a struct, class or enum type, while `Ok` and
`Err` are shadowable prelude value bindings like `print`, `Some` and `None`.
Using either without arguments is an error (`E0106`), since neither is a value.

A `Result` is inspected the same way an enum is:

```skuld
match lookup("host") {
    Ok(text): print(text)
    Err(error): print(describe(error))
}
if let Ok(text) = lookup("host") { print(text) }
if let Err(error) = lookup("nope") { print(describe(error)) }
```

`Ok` and `Err` are the only two variants, so a `match` must cover both or carry
a wildcard (`E0113`). Arm and `if let` bindings are immutable and scoped to their
branch, retaining a managed payload for that scope. `is_ok(): bool` and
`is_err(): bool` query the tag and take no arguments.

The postfix `?` operator propagates errors. `expression?` evaluates to the
success payload, or returns `Err(error)` from the enclosing function without
running the rest of it. It requires an operand of type `Result` (`E0102`),
an enclosing function returning `Result` (`E0102`), and an identical error type
on both (`E0102`); errors are never converted. `?` binds tighter than any
operator, so `read()? + read()?` applies it to each call. An early return through
`?` releases the operand and everything the scope had acquired, exactly like a
written `return`.

`Result` stores a tag and an inline payload, so constructing one does not
allocate, and a value struct cannot contain itself through a `Result`. Copying
retains a managed payload on whichever side is active. Results support neither
equality, printing nor interpolation as a whole; extract the payload instead.
Automatic error conversion, backtraces and recoverable panics are out of scope.

See [examples/results.skuld](examples/results.skuld).

## Arrays — Implemented

```skuld
func main() {
    let numbers: []int = [1, 4, 6, 7, 3]
    let alias = numbers
    alias[0] += 10
    print(numbers[0]) // 11
    print(numbers.len()) // 5
    let rows: [][]int = [[], [1, 2]]
    rows[0] = [3]
}
```

Arrays are homogeneous, heap allocated and reference counted. Their length is
fixed at construction. Assignment, arguments and returns share the array;
`let` prevents rebinding but permits writing an element, like class fields.
Reading a struct element copies its value; reading a class or array element
shares that reference. Elements can have any implemented non-void value type,
including structs, classes, arrays, options and weak references.

Nonempty literals infer an element type locally. Empty `[]` needs an expected
array type from an annotation, assignment, field, parameter, enclosing array
or return type. Mixed element types and implicit numeric conversions are
rejected. Trailing commas are accepted. No general or bidirectional inference
across unrelated expressions is promised.

`values[index]` requires an `int`. Reads and writes check both bounds, including
negative indexes, and trap with `array index out of bounds` on failure.
`values.len(): int` returns the length and takes no arguments. Indexing binds
with calls and member access and continues across newlines. Array elements
and index expressions evaluate left to right; compound writes snapshot the old
element before evaluating the RHS.

Dynamic array mutation and growth:
- `values.push(element: T)` appends an element, growing geometric capacity.
- `values.insert(index: int, element: T)` inserts at `0 <= index <= len()`, shifting later elements. Traps on invalid index.
- `values.pop(): Option<T>` removes and returns the last element, or `null` if empty.
- `values.remove(index: int): Option<T>` removes and returns element at index, shifting elements left, or `null` if out of bounds.

Slicing, array equality, sorting, callbacks and `for` iteration remain
planned. Strong cycles through classes and arrays still require explicit
breaking or weak class links; arrays themselves cannot be weakened yet.

## Enums and pattern matching — Implemented

Enums are user-declared sum types with optional per-variant payloads:

```skuld
enum Status {
    Pending,
    Active(int),
    Cancelled
}
```

- Enum declarations define a new type in the type namespace.
- Variants may be unit variants (`Status.Pending`) or payload variants (`Status.Active(42)`).
- Variant constructors live in the enum's member namespace: `Status.Pending` constructs a unit variant, and `Status.Active(value)` constructs a payload variant.
- Variants can be separated by commas, newlines, or both. Duplicate variant names are rejected (`E0202`).
- Direct recursive enum variants by value (such as `enum List { Cons(List), Nil }`) are rejected as value cycles (`E0103`); indirect recursion via arrays (`[]List`) or classes is supported.
- Enums have value semantics. Managed payloads (strings, arrays, classes) are automatically reference-counted with retain and release in C codegen.
- Enum values implicitly wrap into `Option<Enum>` where expected.

Pattern matching is performed using the `match` statement:

```skuld
match status {
    Status.Pending: return 0
    Status.Active(code): {
        print(code)
        return code
    }
    _: {
        return -1
    }
}
```

- Target expression must be an enum or a `Result` (`E0102` if not); a `Result` matches as a two-variant enum with `Ok` and `Err`.
- Arm patterns support variant patterns (`Status.Pending`, `Status.Active(code)`) and the wildcard pattern (`_`).
- Arm separator accepts `:` or `->`.
- Arms can have a single statement or a block `{ ... }`.
- Variant payload bindings introduce an immutable local variable scoped to that arm's body.
- Exhaustiveness is strictly checked: every variant must be covered, or a wildcard `_` must be present (`E0113`).
- If every arm returns (or diverges), the `match` statement satisfies the function's return contract.
- Inside loops, `break` and `continue` inside match arms naturally bind to the enclosing loop.

## Demonstration proposals — Experimental

`test.skuld` may contain incomplete examples and comments asking for redesign.
Read the current file before syntax work; do not treat it as a passing fixture
or automatically implement every construct it contains.

- Global variable declarations and calls in the sketch demonstrate a possible
  source layout. Executable global statements are still rejected, and
  `func main()` remains the entrypoint. Adopting implicit entrypoints or
  module initialization requires a separate decision. The root sketch also
  omits required field initializers in `new User()`; executable examples supply
  all fields and run statements inside `func main()`.
- Array literals such as `[1, 4, 6, 7, 3]` and the type syntax `[]int` are now
  implemented; the sketch's global placement is still unsupported.
- `numbers.sort((a, b) => a - b)` is explicitly marked for revision by the user.
  It is not the approved final sorting or callback syntax. Callback typing,
  lambda syntax, in-place versus copying sort and the sorting API remain open.
- No proposal introduces null/undefined values, JavaScript coercions or a
  requirement to match TypeScript. Strong typing and Skuld's own design goals
  continue to govern these decisions.

## Later capabilities — Planned

Interfaces and `impl Printable for User { ... }` remain planned. Their member
and receiver syntax needs alignment with the class design before it is fixed;
the earlier `func print(self)` sketch is superseded as a class-method model.
Enums are sum types, e.g.
`enum Status { Online Offline Away }`.
`Option<T>` with `Some`/`None` and `Result<T, E>` with `Ok`/`Err` and `?`
propagation are implemented above.

Future FFI: `extern "C" { func puts(text: *char) -> int }`.
Future commands: `new`, `fmt`, `test`, `doc`. LLVM/Cranelift and eventual
self-hosting remain long-term possibilities.

`ROADMAP.md` proposes the order in which these capabilities would arrive —
enums and `match`, then `for`, then `Result` and `?`, then bytes and string
slices, then the FFI — together with the design questions each one depends on.
The first three have landed; the remainder is a plan, not a commitment, and
none of it is implemented.

## Unsupported features and experimental status

**Experimental:** the demonstration proposals above are documentation-only;
no experimental compiler features are enabled. The full native pipeline, functions, scalar values,
variables, conditional execution and classes with methods/interpolation
(Demos 0–3) are **Implemented**.

Loops (`while`, `loop`, `for`), structs, classes, interpolation, weak class references, arrays, Option,
`Result` with `?`, enums, pattern matching and reference-counted runtime behavior are **Implemented**. Interfaces,
modules and the remaining capabilities above are **Planned**. No generics, macros, async/await, threads, channels,
reflection, decorators, annotations, package registry, compiler plugins,
compile-time execution, operator overloading or user-defined conversions will
be implemented before Demo 3. Inheritance is excluded from the core design.
