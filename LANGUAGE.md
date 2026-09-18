# Skuld language specification

Status labels: **Implemented** means available now; **Planned** describes future
intent, not accepted/executable programs; **Experimental** denotes provisional
choices. The complete single-file native pipeline, Demos 0–2, loops, structs,
reference-counted strings, interpolation, classes, weak references, arrays,
builtin Option values, builtin `Result<T, E>` with `?` propagation, the sized
integer types and byte-level string access are implemented. Self-hosting
remains planned.

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
The official `skuld fmt` is implemented and is the authority on style. It
preserves every `//` comment where its author put it, including the blank line
that separates a comment belonging to nobody from the declaration below it.

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
impl enum match import pub extern unsafe interface`. A lambda needs none of them: it is
written `(a: int): int { ... }`, the shape a method already uses.
Reserved future keywords: `static`.
`true` and `false` produce boolean literal tokens. Type names, `print`, `Some`,
`None`, `Ok` and `Err` are identifiers. `Option<T>` and `Result<T, E>` are
builtin type syntax, not user-defined generics. In a type annotation,
`Option<int>=None` and `Result<int, string>=value` separate the closing `>`
from assignment even though the lexer otherwise recognizes `>=` as one operator.
`>>` is parsed as closing tokens in generic types and as a shift operator in
expressions, so nested generic types close cleanly without special lexer modes.
Recognizing a keyword does not implement its syntax or semantics.

Delimiters: `( ) { } [ ] , . .. : -> =>`.
Operators: `+ - * / % & | ^ ~ << >> = == != < > <= >= ! ? && || += -= *= /= %= &= |= ^= <<= >>=`.
Bit operators: `&` (AND), `|` (OR), `^` (XOR), `~` (bitwise NOT), `<<` (shift left),
`>>` (shift right) and their compound assignments work on all integer widths.
Bitwise operators bind tighter than comparisons (`==`, `!=`, `<`, `>`, `<=`, `>=`).
`>>` is arithmetic on signed types and logical on unsigned types. A shift count that
is negative or greater than or equal to the type width causes a runtime trap.
`?` is postfix and only valid after an expression; see error handling below.
Operators use longest matching; a sign is separate from a number.

Integers are decimal `[0-9]+`, hexadecimal `0x` / `0X`, binary `0b` / `0B` or
octal `0o` / `0O`, stored as `u64` magnitudes. The digit separator `_` is allowed
in any base (e.g. `1_000_000`, `0xFFFF_0000`, `0b1010_0101`). Floats are
`[0-9]+ '.' [0-9]+`, stored as finite `f64`. Leading zeros in decimal are valid.
Integer magnitudes above `u64::MAX` and non-finite float results are lexical
errors. Signed `int` range checking is deferred to semantic analysis, so the
magnitude in `-9223372036854775808` remains representable. Decimal float parsing
uses normal f64 rounding. A dot without a digit on both sides is a separate
token, e.g. `1.foo`, `.5`, and `1.`. Exponents and numeric suffixes are not
supported; they can lex as adjacent tokens, which the parser rejects in a numeric expression.

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

Semantic types: `int = i64`, `float = f64`, `bool`, `char`, `string`, `void`, the
sized integers `i8 i16 i32 i64` and `u8 u16 u32 u64`, and the word-sized integers
`isize` and `usize`. Sized integers are platform independent; `isize` and `usize`
match the target pointer width (`ptrdiff_t` and `size_t` in C) and are distinct
types rather than aliases of `i64` or `u64`. `int` and `i64` are two spellings of
one type rather than two types with a conversion between them, so a value of one
is a value of the other. Semantic types use an enum, never source spellings.
`char` is an unsigned Unicode scalar value whose literal is `'a'`.

An integer literal takes the width its context expects and is range-checked
there, so `let b: u8 = 256` is rejected where it is written rather than
truncated. Without a context a literal is an `int`. A signed type's most
negative value is written as a minus applied directly to a literal
(`let a: i8 = -128`), which is the one place a magnitude one past the positive
range is accepted.

Widths never mix implicitly, in either direction. Converting is explicit and
spelled as a call on the target type's name:

```skuld
let byte: u8 = 200
let wide: int = int(byte)
let back: u8 = u8(wide)
let word: usize = usize(wide)
let f: float = float(wide)
let truncated: int = int(3.7) // 3
let c: char = 'a'
let code: u32 = u32(c)
let back_char: char = char(code)
```

Every such conversion is range-checked at run time and traps when the value
does not fit, in keeping with trapping arithmetic. `int(x)` and `i64(x)` are
the same conversion. Converting floating-point to integer (`int(f)`, `i8(f)`, etc.)
truncates towards zero and traps on out-of-range values or NaN. `char(v)` traps
if the integer is not a valid Unicode scalar value.

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

`const` declares a compile-time constant at module or function scope; `pub const`
exports it from a module. Constant expressions are evaluated at compile time
over scalar/string literals, arithmetic, bitwise operators, string concatenation,
and explicit type conversion calls. Constants allocate no runtime stack slots
or registers and are inlined directly at lowering:

```skuld
const MAX_BUFFER: int = 1024
pub const PROTOCOL_VERSION: int = 1

func compute() -> int {
    const LOCAL_FACTOR: int = 2
    return MAX_BUFFER * LOCAL_FACTOR
}
```

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

Numeric operators require two operands of the same numeric type, which for
integers means the same width as well. `+ - * /` work on every integer width
and on float; `%` works on every integer width. Arithmetic traps on overflow
and on invalid division at every width rather than wrapping, and unary `-` is
rejected on an unsigned type, where only zero would have a result. Bitwise
operators `&`, `|`, `^`, `<<`, `>>` and compound assignments `&=`, `|=`, `^=`,
`<<=`, `>>=` work on every integer width, as does bitwise NOT `~`. Bitwise
operators never mix integer widths implicitly. `<<` shifts left; signed shift left
guards against signed overflow UB. `>>` shifts right: arithmetic on signed integers,
logical on unsigned integers. Shift amounts must be non-negative and strictly less
than the integer width in bits, or the runtime traps with "shift amount out of range".
The left operand of a binary expression supplies the expected width to the right one,
so `byte * 2` or `byte << 2` types the literal as the left operand's width; put the typed
operand first, or annotate, when both sides could be literals. Numeric ordering
returns bool. Equality and inequality work on matching int, float, bool or
string values. Strings
compare byte content, not pointer identity. `&&`, `||` and `!` require bool;
there is no truthiness or implicit int/float conversion. Unary `+` and `-`
require numbers; unary `~` requires integers. Function values, chars, invalid member accesses and unknown types produce
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
Record construction names every field without a default exactly once: there is
no partial initialization, so a missing field is `E0112` and a repeated one is
a duplicate declaration. Field order in construction is free; the backend lays
fields out in declaration order.

## Field defaults — Implemented

```skuld
class Account {
    owner: string = "unnamed"
    tags: []string = []
    balance: int = 0
}

func main() {
    print(new Account().balance)
    let ada = new Account(owner: "Ada", balance: 120)
    let origin = Vec2 { x: 0.0, y: 0.0 }
}
```

A field written `name: Type = expression` may be left out of a construction,
and the expression is evaluated there, once per object — two objects never
share a defaulted array. Classes and structs both take them, and a struct whose
fields all default is written `Point {}`.

There is no zero value and no `undefined` behind this: a field either carries a
default its author wrote or has to be supplied, so an object is always fully
initialized.

A default is an expression of the file that declares the type, not code inside
it. `this` is not in scope, and neither is another field: there is no object at
the point a default runs. Using either is `E0201`, with a help line saying so.

Evaluation order is the usual one. The arguments that were written are
evaluated first, left to right, and then the defaults of the fields nobody
wrote, in declaration order.

There is no constructor with a body. One would expose a `this` whose fields are
not all set, which construction guarantees against; initialization that can
fail is therefore an ordinary function returning a `Result`.

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

## Bytes and slices — Implemented

A Skuld string is a sequence of bytes, and this milestone makes that reachable
without adding a character or code point type.

```skuld
let text = "olá"
print(text.len())        // 4: bytes, not characters
print(text[0])           // 111, a u8
print(text[0..2])        // "ol"
let bytes = text.bytes() // []u8
```

- `text.len(): int` is the byte count, which is what indexing and slicing
  address.
- `text[index]` reads one byte as a `u8`. The index is an `int` and is checked;
  an out-of-range index traps.
- `text.bytes(): []u8` copies the bytes into an array.
- Strings are immutable, so `text[index] = value` is rejected (`E0204`). Build a
  `[]u8` and convert it instead.

`value[start..end]` slices a string into a string or an array into an array of
the same element type. Both endpoints are required and are `int`; the range is
half-open, like every other range in the language, so `text[5..5]` is empty and
`text[0..text.len()]` is the whole. A reversed or out-of-range slice traps.

A slice copies rather than retaining what it came from: a three-byte view must
not keep a large buffer alive. String literals are the exception, because their
bytes are static and outlive every slice of them, so slicing a literal is free
and observably identical. Slicing an array copies its elements, retaining any
that are managed, which makes the result a genuinely separate array: pushing to
a slice does not touch the original.

Going from bytes back to a string is fallible, because arbitrary bytes are not
text:

```skuld
match bytes_to_string(bytes) {
    Ok(text): print(text)
    Err(reason): print(reason)
}
```

`bytes_to_string(bytes: []u8) -> Result<string, string>` validates strict
UTF-8, rejecting overlong encodings, surrogate halves and anything above
U+10FFFF. The error side is a message naming the offending byte offset. That a
message stands in for a dedicated error type is **Experimental**: no error enum
belongs in the language before a standard library exists to own one.

Building a string is building a `[]u8` and validating it once at the end;
`push` and the other array operations are the string builder. Encodings other
than UTF-8, a `char` or code point type, normalization and regular expressions
remain out of scope.

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

## Fixed-size arrays — Implemented

Fixed-size arrays allocate contiguous elements inline on the stack, inside a struct, or inline in a class allocation without heap allocations:

```skuld
var line: [256]u8 = [0; 256]
let header: [4]u8 = [0x7F, 'E', 'L', 'F']
```

- **Type syntax:** `[N]T`, where `N` is a constant integer expression `> 0`, and `T` is any value or reference type.
- **Literals:**
  - Repeated-element literal: `[element; count]`, where `count` is a constant integer expression `> 0`.
  - List literal: `[a, b, c]`, inferred as `[N]T` when an expected fixed array type of matching length is present.
- **Semantics:**
  - Value semantics: assigned or passed by value, copying all elements. Mutating an element (`arr[i] = v`) requires `var` or reached through a class reference.
  - Indexing: `arr[i]` checks bounds both at compile time (for constant indices) and at runtime, trapping with `array index out of bounds`.
  - Length: `arr.len()` is a compile-time constant int.
  - Slicing: `arr[a..b]` creates an owned dynamic array `[]T` copying the sliced elements.
  - Slice coercion: `[N]T` coerces implicitly to `[]T` slice views for functions expecting dynamic slices (e.g. `bytes_to_string(arr)`), with immortal stack lifetime and zero heap allocations. Mutating a coerced slice view (e.g. `push`) traps at runtime with `"cannot mutate a fixed array view"`.
  - Escape prevention: returning a local fixed array as a dynamic slice `[]T` or assigning a fixed array to a class dynamic array field is rejected at compile time (`E0103`).
  - Fields: struct and class types can embed fixed arrays inline (`field: [N]T`).
  - Iteration: `for item in arr { ... }` loops over elements by value copy.
  - FFI: `ptr(arr)` borrows `*T` for scalar fixed arrays directly without copying.

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

- Target expression can be an enum, a `Result` (which matches as a two-variant enum with `Ok` and `Err`), or any scalar or string type (integers, float, bool, string). Non-matchable types like structs or classes are rejected (`E0102`).
- Arm patterns support variant patterns (`Status.Pending`, `Status.Active(code)`), literal and constant value patterns, half-open ranges `a..b`, inclusive ranges `a..=b`, and the wildcard pattern (`_`).
- Arm separator accepts `:` or `->`.
- Arms can have a single statement or a block `{ ... }`.
- Variant payload bindings introduce an immutable local variable scoped to that arm's body.
- Exhaustiveness is strictly checked: every enum variant must be covered or a wildcard `_` provided; for scalar and string matches, a wildcard `_` arm is mandatory (`E0113`).
- If every arm returns (or diverges), the `match` statement satisfies the function's return contract.
- Inside loops, `break` and `continue` inside match arms naturally bind to the enclosing loop.

## Foreign functions — Implemented

An `extern "C"` block declares functions that another object file defines.
Nothing else about the language changes: a foreign function is called like any
other, and the compiler emits its declared name for the linker to find.

```skuld
unsafe extern "C" {
    func write(fd: i32, buffer: *u8, count: u64) -> i64
    func abs(value: i32) -> i32
}

func main() {
    let text = "hello from libc\n"
    let written = write(1, ptr(text), u64(text.len()))
    print(abs(-3))
}
```

`unsafe` is required on the block. The compiler cannot check a declaration
against the library that is eventually linked, so the signature is an assertion
by whoever writes it, and getting it wrong is undefined behavior in C rather
than a Skuld diagnostic. The marker is where that responsibility is recorded.
Only `"C"` is a supported ABI, and a declaration carries no body.

**Managed values never cross the boundary.** C knows nothing about retain and
release, so a signature accepts only:

- the scalars `i8 i16 i32 i64 u8 u16 u32 u64 isize usize`, `float`, `bool` and `char`,
- raw pointers, and
- `void`, as a return type.

A `string`, an array, a class, an `Option` or a `Result` in a signature is
rejected (`E0103`), as is a pointer to one. The declared name may not be `main`
or start with `skuld_`, which the generated program already uses.

`*T` is a raw pointer, where `T` is a scalar or `void`. It is unmanaged: it
keeps nothing alive, and outside an `unsafe` block Skuld cannot read or write
through it. `*void` is the opaque handle a C library hands back. Reading and
writing through a pointer is described under [unsafe blocks](#unsafe-blocks-and-pointers--implemented).

`ptr(value)` borrows the bytes a `string` or an array of scalars already owns
and yields `*u8` for a string, or `*T` for a `[]T`. It retains nothing: the
pointer is valid only while the value it borrows from is alive, which for a
temporary lasts to the end of the enclosing block. `ptr` is a prelude binding
like `print` and can be shadowed. Pass the length alongside it — C has no idea
how long the buffer is, and a Skuld string is **not** NUL-terminated, so a
function that expects a C string needs a `[]u8` with an explicit trailing `0`.

Libraries beyond libc are selected on the command line, not in the source:

```bash
skuld run program.skuld -lm
skuld build program.skuld -L/opt/lib -lfoo
```

Any pointer converts to `*void` where one is expected, with nothing written at
the call. It is the one pointer conversion that cannot be wrong — `*void`
points at no particular type, so nothing can be read through it and nothing
about the pointee is claimed — and C makes it implicitly for the same reason.

Out of scope, and still out: Skuld functions called back from C, varargs,
pointers to pointers, and any ABI other than C. A struct crosses by value when
it declares its layout, which is what `extern struct` is for. Pointer
arithmetic and reading through a pointer arrived with `unsafe` blocks, below,
and neither is available outside one.

A declaration whose C prototype disagrees with a header the generated program
already includes is a clang error at build time, not a Skuld diagnostic. That
is worth knowing before writing one: the generated program includes the C
standard headers, so `system`, `getenv`, `fread` and their neighbours cannot be
declared here at all — their prototypes take `char *` or `FILE *`, neither of
which Skuld can name. The POSIX functions those headers do not declare —
`open`, `read`, `write`, `close`, `socket`, `fork` — are free to declare, which
is how `std/fs`, `std/net` and `std/os` are written. Two modules of one program
may declare the same C function, as `std/fs` and `std/os` both declare `read`,
as long as they declare it identically.

## Freestanding Skuld — Implemented

A program with no runtime and no libc under it, which is the shape a kernel, a
bootloader or a static utility has:

```bash
skuld check --freestanding kernel.skuld
skuld emit-c --freestanding kernel.skuld > kernel.c
skuld build --freestanding kernel.skuld -o kernel.o
```

**It is the same language.** There is no second dialect and no separate
compiler: what changes is what there is to run, so the checker refuses the
values that would need a runtime and everything else is written exactly as it
is written in a hosted program.

- **What a freestanding program holds:** the scalars, fixed arrays, structs,
  enums, unions, pointers, `const` and `static`. A `string`, a `[]T`, a class,
  an interface, a `weak` reference, and any `Option`, `Result`, struct or enum
  that holds one are refused where they are written — they are reference
  counted, and there is no runtime to count them with and no allocator to take
  them from. `print` is refused too: there is no standard output.
- **It has no entry point of its own.** Nothing generates `main`, and `main` is
  not looked for. A `pub func` is emitted under the name it was written with,
  so whatever starts the program — an assembly stub, a bootloader, another
  object file — has something to call. Everything not `pub` keeps a generated
  name.
- **A build produces an object file**, not an executable: `-o` names it, and
  the default is the source's name with `.o`. Nothing is linked, and no linker
  argument is accepted — the linker script, the target and the startup stub
  belong to whoever is assembling the thing this is part of. For a target that
  is not this machine, `emit-c --freestanding` hands over the C and the C
  compiler is driven directly, which is what the 32-bit kernel test does.
- **The checks stay.** Every index is still bounds-checked and every
  arithmetic overflow still traps; what changes is what a trap can do. There is
  no standard error to explain itself to, so a trap is `__builtin_trap()` — an
  instruction the processor refuses.
- **The generated C includes only what C guarantees a freestanding
  implementation**: `stdint.h`, `stdbool.h`, `stddef.h` and `float.h`.

What a freestanding program cannot do without help is exactly what the language
deliberately cannot spell: the `syscall` instruction, `in`/`out`, and anything
else that is one instruction rather than a value. Those are written in assembly
and declared as `extern "C"`, which is the same boundary the FFI already is.

Memory is the same story. There is no allocator, because nothing allocates: a
freestanding program's storage is its statics and its stack, and a program that
wants more manages a region itself — the compiler stays out of it.

## `static` — Implemented

`static` is storage that outlives every call:

```skuld
static counter: int = 0
pub static histogram: [16]int = [0; 16]
```

- It is written like a constant and behaves like a variable: readable and
  writable from any function in the module, and `pub` to export it.
- **The initialiser is evaluated at compile time.** There is no moment before a
  program starts at which one could run, so a static starts at a constant
  expression — a literal, a `const`, arithmetic over them — and never at the
  result of a call.
- What it may hold is a scalar, or a fixed array of scalars that starts at
  zero. A `string`, an array, a class or an `Option` holding one is refused:
  nothing would retain a managed value that outlives every call, and nothing
  would ever release it. A fixed array starts at zero because C has no repeated
  initialiser, and a written-out one is not an initialiser a reader wants to
  read; fill it with anything else while the program runs.
- A local of the same name shadows it, as a local shadows any module name.
- There are no threads in Skuld, so a static needs no atomics and gets none.

## `defer` — Implemented

`defer statement` runs the statement when the block it was written in is left,
however it is left:

```skuld
let handle = open_file(path)?
defer close(handle)
```

- Deferred statements run in **reverse** order of registration: the last one
  written is the first one run.
- They run on every way out — falling off the end, `return`, `break`,
  `continue`, a `?` that propagates, and the escape block of a `let ... else`.
  They do not run on a trap: a trap ends the process, and there is no
  recoverable panic to unwind.
- A `defer` in a loop body belongs to that iteration, so it runs at the end of
  each one rather than at the end of the loop.
- It is a statement that is **run at the exit**, not a call recorded at the
  `defer`. It reads what its names are worth when the block ends, so a `defer`
  written next to a counter reports the counter's final value. (Go records the
  arguments instead; this follows Zig, which is the behaviour that matches what
  the line looks like.)
- The value a `return` hands over is computed before the deferred statements
  run, so a `defer` can never change what the caller receives.
- `return`, `break`, `continue` and `?` are **refused** inside a deferred
  statement: it runs while the block is already being left, so leaving again
  would have to decide what happens to the exit already under way. A deferred
  declaration is refused too — it would bind a name at the moment the block
  ends, where nothing can read it.
- There is no `errdefer`. `Result` already makes the failure path visible at
  the call site, and where a resource is released on failure but handed over on
  success, a flag says which happened:

```skuld
var handed_over = false
defer {
    if !handed_over {
        socket.close()
    }
}
// ... every failure path just returns ...
handed_over = true
return Ok(connection)
```

`std/net` and `std/tls` are written that way: opening a TLS connection acquires
a socket, a context and a connection, each of which had to be released on every
later failure, and now is released once.

Out of scope: destructors a user can write, cleanup attached to a type rather
than to a scope, and `Drop`-style traits.

## Layout and the ABI — Implemented

An ordinary `struct` is laid out by the compiler, and that layout is
deliberately unspecified: nothing outside the program may depend on it. A type
that describes memory somebody else defined says so, and is then laid out the
way the platform's C compiler lays out the same fields:

```skuld
extern struct SockaddrIn {
    family: u16
    port: u16
    address: u32
    zero: [8]u8
}

extern struct Tagged packed {
    tag: u8
    value: u32
}

extern struct Page align 4096 {
    bytes: [4096]u8
}
```

- **Fields** must be things C can describe: the scalars, a raw pointer, a fixed
  array of those, and another `extern struct`. A string, an array, a class, an
  `Option` or a `Result` is refused — a managed field would put a reference
  count inside a layout C decides, where nothing would retain or release it.
- **`packed`** removes the padding between fields; **`align N`** aligns the
  whole type to at least `N` bytes, where `N` is a power of two up to 4096.
  Both are written between the name and the body, and both are ordinary
  identifiers everywhere else in the language.
- **Crossing the boundary.** An `extern struct` may be a parameter and a return
  type of an `extern "C"` function, passed and returned by value the way C
  does it. `ptr(value)` takes the address of one as a `*void`, which is what a
  call like `connect` expects. Nothing else about the boundary changed: a
  managed type is still refused.
- **`size_of(Type)`** and **`offset_of(Type, field)`** answer in bytes, as a
  `usize`. Both are refused for a type whose layout the compiler chose, since
  there is no answer a program is allowed to depend on. Neither argument is a
  value: one names a type and the other a field. The answer comes from the C
  compiler for the target being built, so it is right on every target rather
  than recomputed by Skuld — which also means it is not a compile-time constant
  and cannot initialize a `const`.

### Unions

An `extern union` is one piece of memory read as one of several types:

```skuld
extern union Word {
    whole: u32
    halves: [2]u16
    bytes: [4]u8
}
```

- It is constructed one member at a time — `Word { bytes: [0; 4] }` — and
  writing a member is what makes that member the live one, which needs no
  claim.
- **Reading** a member is written inside `unsafe`. A union says nothing about
  which member was last written, so a read is a claim the program makes and not
  something the compiler knows.
- Its members follow the same rule an `extern struct`'s fields do, and it is a
  value like any other: it copies, it can be a field, and it can cross the
  boundary.

### Numbered enums

An enum may name the integer type its variants are worth:

```skuld
enum Protocol: u8 {
    Icmp = 1,
    Tcp = 6,
    Udp = 17,
}
```

- A variant that writes no value continues from the one before it, starting at
  zero, the way C numbers an enumeration. Two variants worth the same number
  are refused: the conversion back would be ambiguous and one of them
  unreachable.
- `u8(protocol)` — or any width the value fits — converts a variant to its
  number, and `Protocol(value)` converts back, trapping on a number no variant
  is worth. The trap is the point: the declared numbers are the only protocols,
  so a number read from somewhere else is checked before the rest of the
  program matches on it.
- Only an enum without payloads can be numbered: a variant that carries
  something is not worth an integer.
- A numbered enum is not itself a foreign type. It crosses the boundary as the
  integer it converts to, which is written where a reader can see it.

`std/net` and `std/dns` are written this way: `SockaddrIn` and `Timeval` are
declared types rather than hand-packed bytes, and `struct timeval` is why it
matters — its two fields are `long`, so it is sixteen bytes on a 64-bit target
and eight on a 32-bit one.

Out of scope: bitfields with C's allocation rules, a guarantee about the
ordinary layout, and a layout that varies by target in the same source.

## `unsafe` blocks and pointers — Implemented

An `unsafe` block is where the guarantees the compiler makes everywhere else are
suspended, and says so in the source:

```skuld
unsafe {
    var register: u32 = 0
    let port = ptr(register)
    volatile_store(port, u32(1))
    print(int(volatile_load(port)))
}
```

Everything outside such a block keeps every rule it had: no unchecked index, no
aliasing of a managed value, no pointer read at all. The block changes nothing
about the generated code — it is a claim the programmer makes, and the only
thing the compiler does with it is stop refusing the operations below.

| Operation | Meaning |
| --- | --- |
| `load(pointer)` | The value at `pointer`, of the pointer's own pointee type |
| `store(pointer, value)` | Writes `value` through `pointer` |
| `volatile_load(pointer)` | A read the backend may neither drop nor reorder against another volatile access |
| `volatile_store(pointer, value)` | The same for a write |
| `offset(pointer, count)` | Steps `count` elements, not bytes |
| `addr(pointer)` | The address, as a `usize` |
| `ptr_from(address)` | The pointer an address names; its type comes from the context |
| `ptr(local)` | The address of a scalar local |

- Nothing new spells a load. Its type is the pointer's pointee, so `load(p)`
  over a `*u32` is a `u32`; there is no `load<u32>(p)` form and no `*p`
  operator. `ptr_from` is the one that reads the expected type from its
  context, as `None` and an integer literal already do, and it is an error
  where that context does not name a pointer type.
- These are prelude bindings like `print` and `u8()`, and can be shadowed the
  same way.
- A pointer still points only at a scalar or `void`. A `*string`, a `*[]u8` or
  a pointer to a class stays a diagnostic: loading one would hand the reference
  counter a value it never saw allocated. `*void` points at no particular type,
  so `load`, `store` and `offset` all refuse it.
- A pointer to a pointer is still not a type. An address travels as a `usize`
  and comes back through `ptr_from`, which is how a linked structure stores its
  links.
- `ptr(local)` takes the address of a scalar `var`. A `let` is refused, since a
  pointer can always write through it, and a parameter is refused, since its
  address is the address of a copy.
- An `unsafe` block is lexical, not dynamic: a function called from inside one
  is not itself inside it, and needs its own block.
- The foreign boundary does not widen: an `extern "C"` signature still refuses
  managed types, and `ptr()` over a string or an array still borrows rather
  than escapes.

Out of scope: references with lifetimes, aliasing rules, a borrow checker, and
unchecked indexing — `load` and `store` are the only unchecked accesses in the
language, and each one is written where a reader can see it.

## Function values — Implemented

A lambda is a function declaration without a name, which is why it needs no
keyword: a method already declares itself the same way.

```skuld
let increment = (n: int): int { return n + 1 }
let double = (n: int) => n * 2
numbers.sort((a, b) => a - b)
```

A lambda can have a block body or an expression body with `=>` (e.g. `(a, b) => a - b`).
Where expected context supplies parameter and return types, annotations may be omitted.

A function *type* is written `(int, int) -> int`. The result uses `->` there so
that a parameter is not spelled `compare: (int, int): int`, with `:` meaning
"has type" and "returns" in the same declaration; inside a literal both `:` and
`->` introduce the result, as they do on a method.

```skuld
func count_if(values: []int, keep: (int) -> bool) -> int {
    var total = 0
    for value in values {
        if keep(value) {
            total = total + 1
        }
    }
    return total
}
```

A parameter type may be omitted when the expected type supplies it — the same
local inference `let` performs — and a declared function named where a value is
expected becomes one:

```skuld
print(count_if(numbers, (n): bool { return n > 1 }))
print(apply(10, double))
```

**A function value may not escape.** It can be a parameter or a local, and
never a field, a return type, an array element, or a payload of `Option`,
`Result` or an enum. This is the rule the rest depends on, so it is worth
stating why it exists rather than treating it as a restriction: Skuld reference
counts with non-atomic counts and has no cycle collector, so any managed object
able to reach a closure that captured it would be a cycle nothing frees.
Removing the reachability is cheaper than asking every user to reason about
weak captures, which is the ownership burden the language sets out to avoid.

What it buys is that a function value costs nothing. It never allocates, never
retains and never releases: its captures are copied into an environment that
lives in the enclosing block, which the value cannot outlive.

**A lambda captures values, not variables.** Only immutable bindings — `let`,
parameters, `this`, and the bindings introduced by `if let`, `match` and `for`
— may be captured. A copy of a `var` could disagree with the variable by the
time the value runs, and copying is exactly what makes the environment free, so
the two rules are one rule.

```skuld
let base = 100
print(apply(5, (n: int): int { return n + base }))   // captures `base`

var running = 0
print(apply(5, (n: int): int { return n + running })) // rejected
```

**A chosen limit.** Because a function value cannot be stored, a callback
cannot be kept for later: there is no handler table, no registry and no
observer list. That is deliberate, not an oversight — those shapes want a named
abstraction, which is what interfaces are for, and interfaces are a milestone
of their own with receiver syntax still unsettled.

One parsing rule follows from the syntax: a lambda body opens on the same line
its parentheses close. Newlines are not tokens, so without that rule
`var x = (None)` followed by a block on the next line would read as a lambda. A
parenthesised condition, as in `if (flag) { ... }`, is unaffected — conditions
already refuse a bare record literal, and they refuse a lambda for the same
reason.

`sort()` is the first consumer. It sorts in place and returns `void`, like
`push`, `insert`, `pop` and `remove`, because an array is a shared reference.
`to_sorted()` returns a newly allocated sorted copy (`[]T`), leaving the original
array untouched. Both methods are stable and run on a snapshot: a comparator that
changes the array while the sort is running aborts rather than reading a buffer the
array no longer owns.

```skuld
var words = ["pear", "fig", "banana", "kiwi"]
words.sort((a, b) => a.len() - b.len())
// fig, pear, kiwi, banana — `pear` and `kiwi` keep the order they were in

let numbers = [5, 3, 9, 1]
let sorted = numbers.to_sorted((a, b) => a - b)
// numbers remains [5, 3, 9, 1]; sorted is [1, 3, 5, 9]
```

A comparator returns a negative, zero or positive `int`. Writing that as
`a - b` is the usual shorthand and it is only safe when the values are small:
arithmetic traps on overflow at every width, so comparing values near the
extremes of `int` that way aborts the program. Comparing and returning `-1`,
`0` or `1` always works.

## Interfaces — Implemented

An interface names a set of method signatures. A class states which ones it
implements on its own declaration:

```skuld
interface Renderer {
    render(value: int) -> string
    label() -> string
}

class Decimal: Renderer {
    prefix: string

    render(value: int) -> string {
        return "${this.prefix}${value}"
    }

    label() -> string {
        return "decimal"
    }
}

func show(renderer: Renderer, value: int) {
    print("${renderer.label()}: ${renderer.render(value)}")
}
```

An interface method needs no new spelling: a class method already takes an
implicit `this`, so a signature is that method without its body.

**Conformance is declared, never inferred.** A class with the right methods
that never said so is not accepted, and each declared interface is checked
method by method, with parameters and result matching exactly. Nothing is
coerced to fit; a near miss names both signatures. Saying it on the
declaration is the same choice `pub` made in modules: this language says what
it means where the thing is defined.

**Only a class implements an interface.** A struct is a value with no
identity, so putting one behind an interface would mean boxing it — an
allocation and a lifetime question with nothing asking for them. A class is
already a counted reference, so an interface value is that reference plus a
table of methods, and reference counting works unchanged.

**An interface value is storable**, which is the point. It may be a field, an
array element, a payload or a return type, and it outlives the call that made
it:

```skuld
class Registry {
    handlers: []Handler

    add(handler: Handler) {
        this.handlers.push(handler)
    }
}
```

That is what a function value deliberately cannot do, and it is why the two
exist side by side: a lambda is a cheap, non-escaping argument, and an
interface is a named abstraction you can keep. Keeping one brings the cycles
classes already have, and `weak` is already the answer to those.

A class widens into an interface it declared the way a value wraps into an
expected `Option`, and the two compose in that order, so `Option<Handler>`
accepts a class directly.

**Not here:** inheritance between interfaces, default method bodies, structs
behind interfaces, and asking at run time which concrete class is inside. A
downcast is a decision of its own.

## Unwrapping with an escape — Implemented

A declaration can unwrap an `Option` or a `Result` and say what to do when
there is nothing to unwrap:

```skuld
let response = http.get(url) else reason {
    print(http.describe(reason))
    return
}
let document = json.parse(response.body) else reason {
    print(json.describe(reason))
    return
}
print(json.render(document))
```

`response` is the payload, in scope for the rest of the block. A `Result`
names its error inside the escape block and nowhere else; an `Option` carries
no error, so writing a name there is an error rather than a binding of
something absent — `else { ... }` is the whole form.

**The block must not fall through.** That is the entire rule, and it follows
from the binding outliving the statement: reaching past the block would leave
the name unbound. `return`, `break` and `continue` all leave. An `if` that only
sometimes returns does not, and is refused:

```skuld
let value = fallible() else reason {
    if reason.len() > 0 {
        return
    }
}                       // rejected: `value` would be unbound here
```

Inside a function that returns a `Result`, `?` already does this in one
character. The escape form is for everywhere else — chiefly `main`, which
returns `void` and so has no `?`, and any place where two calls carry different
error types, since `?` never converts between them. It is deliberately not a
way around either rule; it is a way to handle a failure and carry on straight
down the page, which is what reading code wants.

## Modules — Implemented

A program is a set of modules. A **module is a directory**: every `.skuld` file
in it shares one namespace, so splitting a module across files is a filing
decision rather than a semantic one, and two files of the same module call each
other without importing anything.

```skuld
// geometry/point.skuld
pub struct Point {
    x: int
    y: int
}

pub func origin() -> Point {
    return Point { x: 0, y: 0 }
}

// Not exported: visible to the rest of `geometry`, and nowhere else.
func scale(value: int) -> int {
    return value * 2
}
```

```skuld
// main.skuld
import "geometry"

func main() {
    print(geometry.origin().x)
}
```

**Importing.** `import "path"` comes before every declaration in a file, so the
top of a file lists everything it depends on. The path is relative to the
**program root**, the directory the entry file lives in, and its segments are
plain names — `..`, a leading `/` and an empty segment are rejected, so a path
never climbs out of the root. Imports belong to the file that writes them, not
to its module: a sibling file that wants the same module imports it too.

**Qualification.** An import binds one name: the **last segment** of the path.
`import "net/socket"` is spelled `socket.connect(...)`. There is no unqualified
access to another module, so an import can never quietly change what a name in
the file already means, and reading a call says where it comes from. Two
imports whose last segments agree are rejected in that file rather than one
silently winning. An ordinary binding shadows a qualifier like any other name:

```skuld
import "geometry"

func main() {
    let geometry = "an ordinary string"
    print(geometry)
}
```

**Exporting.** `pub` marks what leaves a module — functions, structs, classes
and enums. Visibility is written rather than inferred from spelling; a capital
letter would have decided it in a language whose prelude is `print`, `len` and
`bytes_to_string`. A method is public with the type that owns it, so `pub` is
never written on a method, and a field is reachable wherever its type is. An
`extern` block cannot be exported: a foreign signature is an assertion made by
the module that wrote it.

Types are qualified in every position they can appear:

```skuld
import "geometry"

func place(p: geometry.Point, all: []geometry.Point) -> Option<geometry.Point> {
    match geometry.Shape.Box(p) {
        geometry.Shape.Dot: return null
        geometry.Shape.Box(inner): return inner
    }
}
```

A qualified pattern names its enum by identity, not by spelling, so two modules
may each declare a `Tag` without either shadowing the other.

**Cycles are rejected.** If two modules import each other there is no order in
which they could be checked, and nothing in the language needs one; the
diagnostic points at the import that closes the cycle.

**What a module is not.** There is no package registry, no versioned or remote
dependency, no conditional compilation, and no separate compilation: the
compiler still reads every source of a program on every build. The entry file
is the root module on its own, so a directory full of unrelated programs — as
`tests/pass` is — does not become one module by sitting together.

## The standard library — Implemented

`std` is a **reserved import prefix**, not a directory. Its modules are written
in Skuld and embedded in the compiler binary, so `import "std/utf8"` works from
any directory, needs no installation step, and cannot be replaced by a `std`
directory sitting next to the program — that directory is simply unreachable.
A reserved path naming no module is an error that lists the modules that exist;
it never falls back to disk. Everything else keeps the ordinary rule and
resolves under the program root.

```skuld
import "std/utf8"
import "std/strings"

func main() {
    let line = strings.trim("  HTTP/1.1 200 OK \r\n")
    print(strings.starts_with(line, "HTTP/"))
    match utf8.decode(bytes) {
        Ok(text): print(text)
        Err(error): print(utf8.describe(error))
    }
}
```

The library is deliberately small, and each entry exists because something
already planned calls it. Adding to it is not a licence to build a general
library ahead of its users.

**`std/utf8`** owns the error type the language had no way to name.
`Utf8Error` is `Truncated`, `Overlong`, `Surrogate`, `TooLarge` or `Invalid`,
each carrying the byte offset where the faulty sequence starts; `describe` and
`offset` read it. `validate(bytes) -> Result<int, Utf8Error>` applies the strict
rules and returns the number of code points, `count` is its name when that
count is the point, and `decode(bytes) -> Result<string, Utf8Error>` is what a
caller reaches for.

The builtin `bytes_to_string` is unchanged and still returns
`Result<string, string>`. Making it return a library type would invert the
dependency — the checker would have to know a name the library chose — so the
provisional message stays where it is, and `utf8.decode` is the call with a
real error beside it.

**`std/strings`** works in byte offsets, which is what the language already
speaks: `starts_with`, `ends_with`, `index_of` (an `Option<int>`), `contains`,
`trim`, `split` and `join`. `trim` removes spaces, tabs, carriage returns and
newlines — the set a header field needs, not a Unicode whitespace table, which
would need code points. `split` keeps empty fields, so its result is never
shorter than one element.

**`std/cstring`** closes the gap the FFI left open. A Skuld string is
length-aware and not NUL-terminated, so `to_c(text) -> Result<[]u8,
CStringError>` copies the bytes and appends the terminator, refusing a string
that already contains a NUL, since a C string would end there.

**`std/fs`** reads and writes a whole file by path — `read_file`, `read_text`,
`write_file`, `write_text` — which is what a document transformed whole wants,
and answers a `Result` whose error names the step and the path; there is no
`errno`, for the reason `std/net` already records. For a file that will not
fit in memory, `open` and `create` answer a `File` with `read`, `write` and
`close`, where `read` takes at most so many bytes and answers empty at the
end. A handle needs a lifetime rule and Skuld has no destructor a user can
write, so the rule is the caller's and it is one line: `defer file.close()`,
which runs on every way out including a `?` that propagates. That is
deliberate — cleanup belongs to a scope a reader can see rather than to a type
where the second close of a double close would be invisible.

**`std/os`** reaches the process itself: `arguments()` and `parameters()` (the
same list without the program), `flush()` and `exit(code)`. Following `argv`
means reading a pointer to pointers, which the foreign boundary does not do, so
the runtime offers a count, a length and a copy into bytes Skuld already owns.

It also runs other programs. `run(program, arguments)` looks the program up on
`PATH`, waits for it, and answers an `Output` with its exit status, what it
wrote to standard output, and whether that was longer than the 64 KiB kept.
**There is no shell**: nothing is expanded, split or quoted, so an argument
containing a space or an asterisk is one argument and stays literal. A program
that does not exist answers 127, the status a shell reports for the same thing,
and one killed by a signal answers -1. Its standard error is left alone, so a
program that complains still complains where a person can see it. The `argv`
array is the pointer-to-pointer the boundary refuses to describe, so it is
built in raw memory with `store`, and `size_of` over a one-pointer
`extern struct` is how the module asks how wide a pointer is on this target.

`environment(name)` answers the value of an environment variable, or nothing.
It reads `/proc/self/environ` rather than calling `getenv`, whose prototype
takes a `char *` — a type Skuld cannot name, since its own `char` is a Unicode
scalar and not a byte.

**`std/testing`** is what `skuld test` runs: `check`, `equal_int`, `equal_text`,
`equal_bool`, `fail` and `passed`. A failing assertion prints why and ends the
process, because Skuld has no recoverable panic to carry on from.

**`std/map`** is a `StringMap`: `set`, `get`, `has`, `remove`, `len`, `keys`
and `values`, with string keys, integer values and iteration in insertion
order. It is an index rather than a general collection — the value is a
position, a count or an identifier, and what is indexed stays in the array that
holds it. Generics remain out, and choosing a map did not authorize them.

**`std/dns`** resolves a host name to an IPv4 address by speaking DNS over a
`connect`ed UDP socket, because every resolver in libc answers with a pointer
and the foreign boundary does not read through one. Servers come from
`/etc/resolv.conf`, a query times out after three seconds, and there is no
cache. An answer is matched against the question before any of it is believed —
the query id, the question section and the bit that says this is a response —
because a datagram arrives from whoever sent one, and `connect` fixes only the
address and the port. The id is sixteen bits from the system's own generator,
which is the only kind of randomness this library offers: there is no seedable
one to reach for by mistake.

**`std/tls` and `std/https`** are the one place the project takes a
dependency, and it is opt-in: they bind OpenSSL, so a program that imports
them links it itself with `-lssl -lcrypto` — `-llibssl -llibcrypto` under the
MSVC toolchain Windows builds use, since those are the linker's names and not
the language's. `std/http` still refuses `https://` and links nothing. Verification cannot be turned off from Skuld: the chain is
checked against the system trust store and the name against the certificate,
and there is no `insecure` flag.

**What the library is not.** There is no collection beyond arrays, no map, no
time, no randomness and no filesystem traversal. It is not a package registry,
and there is no way to add to it except by changing the compiler — which is the
price of embedding, and a deliberate brake while the library is small enough to
move with the language.

**What it holds**, each entry with a caller in a milestone that asked for it:

| Module | What it is |
| --- | --- |
| `std/utf8` | Strict UTF-8 validation and decoding, with a real error type |
| `std/strings` | `starts_with`, `ends_with`, `index_of`, `contains`, `trim`, `split`, `join` |
| `std/cstring` | Building the NUL-terminated buffer a C function expects |
| `std/json` | Parsing and rendering JSON over `[]u8` |
| `std/net` | A blocking TCP connection over libc sockets |
| `std/http` | An HTTP/1.1 client written on `std/net` |
| `std/fs` | Reading and writing a whole file, by path |
| `std/os` | The process arguments, environment, running a program, a flush, an exit status and `errno` |
| `std/testing` | The assertions `skuld test` runs |
| `std/map` | A map from `string` to `int`, iterated in insertion order |
| `std/dns` | Host names, by speaking DNS over UDP |
| `std/ffi` | The null pointer, and whether a pointer is it |
| `std/tls` | A verified TLS connection over OpenSSL (linked by the program) |
| `std/https` | The HTTP client of `std/http` over `std/tls` |

The last three arrived with M9, and reach as far as fetching a document and
decoding it:

```skuld
import "std/http"
import "std/json"

func main() {
    match http.get("http://127.0.0.1:8080/data.json") {
        Ok(response): {
            match json.parse(response.body) {
                Ok(document): {
                    if let name = json.lookup(document, "name") {
                        print(json.render(name))
                    }
                }
                Err(reason): print(json.describe(reason))
            }
        }
        Err(reason): print(http.describe(reason))
    }
}
```

`std/json` is M4's closing marker promoted out of its fixture: an object is a
list of key/value pairs in source order, so lookup is linear and duplicate keys
are preserved. `std/net` speaks `sockaddr_in` on a little-endian Linux, built
byte by byte rather than through `inet_pton`, because parsing four octets is
arithmetic Skuld can do without another foreign call.

Three limits are worth stating plainly, because each is a consequence of a rule
the language already had rather than a gap waiting to be filled quietly.

**There is no name resolution.** A connection is made to an IPv4 address, never
to a host name. `getaddrinfo` and `gethostbyname` both answer with a pointer to
a structure, and reading through a pointer is out of scope at the foreign
boundary, so no resolver in libc is reachable. `http.get` says so by name when
it is handed one: ``` `skuld.example` is a name, and there is no resolver ```.
Getting names would take either a read primitive at the boundary or a resolver
written in Skuld over UDP — each a decision of its own.

**There is no TLS**, so `https://` is refused rather than attempted. Binding a
system TLS library is a milestone of its own and arguably a dependency-policy
question rather than a technical one.

**There is no `errno`.** A network failure says which step failed — opening,
connecting, sending, receiving — and not why, for the same pointer reason.

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
- `numbers.sort((a, b) => a - b)` in the sketch is settled: expression lambdas
  using `=>` are supported for inline lambdas (`(a, b) => a - b`). In-place sorting
  remains `numbers.sort(cmp)` returning `void`, while non-mutating copy sorting is
  provided by `numbers.to_sorted(cmp) -> []T`. Parameter types may be omitted
  where expected types provide them.
- No proposal introduces null/undefined values, JavaScript coercions or a
  requirement to match TypeScript. Strong typing and Skuld's own design goals
  continue to govern these decisions.

## Later capabilities — Planned

Interfaces are implemented, and `impl Printable for User { ... }` is not how
they are written: conformance is stated on the class, as `class User:
Printable`. The earlier `func print(self)` sketch is superseded twice over —
a method takes an implicit `this`, and an interface signature is that method
without its body.
Enums are sum types, e.g.
`enum Status { Online Offline Away }`.
`Option<T>` with `Some`/`None` and `Result<T, E>` with `Ok`/`Err` and `?`
propagation are implemented above, as are the sized integers, `[]u8`,
string slicing and the `extern "C"` boundary.

Future commands: `new` and `doc` (`fmt` and `test` are implemented). LLVM/Cranelift and eventual
self-hosting remain long-term possibilities.

`ROADMAP.md` records M1–M23 as implemented, including function values,
callbacks, interfaces, blocking TCP/HTTP with JSON, the official formatter
`skuld fmt`, find-references and rename in the editor, and a native command-line
application with its own `skuld test` suite, field defaults at construction, a
string-keyed map, host-name resolution, verified HTTPS, the first measured
performance baseline in `BENCHMARKS.md`, and bitwise operators with integer
literals in binary, octal and hexadecimal with digit separators (M19), along
with expression lambdas and array `to_sorted` (M19), named constants and value
patterns (M20), word-sized integers and float/integer conversion (M21),
fixed-size arrays (M22) and pointers that can be read inside `unsafe` (M23).
Its planned sequence continues with M24. These proposals do not settle their
syntax or authorize implementation. No implementation milestone is active.

## Unsupported features and experimental status

**Experimental:** the demonstration proposals above are documentation-only;
no experimental compiler features are enabled. The full native pipeline, functions, scalar values,
variables, conditional execution and classes with methods/interpolation
(Demos 0–3) are **Implemented**.

Loops (`while`, `loop`, `for`), structs, classes, interpolation, weak class references, arrays, Option,
`Result` with `?`, enums, pattern matching, the sized integers, `[]u8`, string indexing and slicing,
foreign `extern "C"` declarations with raw pointers, modules with `import` and
`pub`, the embedded standard library,
function values, expression lambdas, stable sorting and `to_sorted`, interfaces, `let ... else`,
bitwise operators, integer literal prefixes, digit separators,
fixed-size arrays, `unsafe` blocks with pointer loads and stores, `defer`,
`static` storage, freestanding builds, and reference-counted runtime behavior
are **Implemented**. The future
capabilities listed in the roadmap are **Planned**. No generics, macros, async/await, threads, channels,
reflection, decorators, annotations, package registry, compiler plugins,
compile-time execution, operator overloading or user-defined conversions will
be implemented before Demo 3. Inheritance is excluded from the core design.
