# Skuld language specification

Status labels: **Implemented** means available now; **Planned** describes future
intent, not accepted/executable programs; **Experimental** denotes provisional
choices. The complete single-file native pipeline, Demos 0–2 and loops are implemented.
Classes, managed allocation and self-hosting remain planned.

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
   classes; future ARC must make allocation and reference-management costs
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

Keywords: `func let var return if else while loop break continue class struct
impl`.
Reserved future keywords: `interface enum match import for in static extern`.
`true` and `false` produce boolean literal tokens. Type names and `print` are
identifiers. Recognizing a keyword does not implement its syntax or semantics.

Delimiters: `( ) { } [ ] , . : ->`.
Operators: `+ - * / % = == != < > <= >= ! && || += -= *= /=`.
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
escapes `\\`, `\"`, `\'`, `\n`, `\r`, `\t`, `\0`. Literal line breaks are
rejected. `${name}` is currently ordinary string text, with no interpolation.
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
unary; call/member; primary. Calls and members include `foo(1, 2)`, `user.name`
and `user.greet()`. The parser preserves spans and remains separate from resolution and type
checking. Assignment is right-associative; other binary operators are
left-associative. Unary `+`, `-`, `!` bind below calls and members. Parentheses
produce explicit group nodes. Assignment targets must be identifiers or member
accesses syntactically, but only variable targets are supported semantically
now. Assignments preserve the target type and yield the assigned value.
Comparison chains are parsed left-associatively and then type-checked normally.
Only direct function/builtin calls, optionally grouped, are supported; callable
values, indirect calls and all member operations are rejected by the type checker. Trailing commas are accepted in parameter and
argument lists. Local declarations require an initializer; annotations are
optional, but function parameters require types. AST type references preserve
source names, including unknown names; these are not semantic type values.

Blocks, return, expression statements, variables, `if`/`else` (including
`else if`), `while`, `loop`, `break` and `continue` are parsed. Later:
`for item in items`, `0..10`, `0..=10` and arrays `[1, 2, 3]` with type
syntax `[]int`.

### Loops — Implemented

`while condition { }` requires a `bool` condition, with no truthiness and no
parentheses around it. The body is a child scope like any other block, and the
condition is re-evaluated before every iteration, including after `continue`.

`loop { }` repeats until a `break` leaves it. `break` and `continue` bind to
the innermost enclosing loop; outside any loop they are `E0111`.

A `loop` that no `break` can leave never falls through, so it satisfies a
non-void return type and any code after it is unreachable. Adding a `break`
restores the fall-through path and the return requirement returns with it. A
`while` never satisfies a return type, because its condition may be false on
entry.

## Statement boundaries and parser API — Implemented

The parser reads line breaks from source gaps between byte spans; the lexer
still emits no newline tokens. Expressions greedily continue through operators,
call parentheses and member dots, including across newlines and comments:

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
AST is exposed if any error occurs. Only top-level function declarations are
accepted. Empty files parse successfully but fail full checking without main.
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

A separate parent prelude contains the typed builtin identity `Print`. User
functions and local bindings may shadow `print`; call checking and lowering
use the resolved symbol, not the spelling of the call. The type checker
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
require numbers. Function values, chars, members and unknown types produce
explicit diagnostics rather than reaching code generation.

Evaluation is left to right, including call arguments. `&&` and `||`
short-circuit. Assignment evaluates its RHS and returns the new value;
compound assignment snapshots the target's old value before evaluating its RHS.
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

Current strings contain immutable UTF-8 literal bytes and a length. Values copy
that view; backing literal storage lives for the program duration. Printing and
equality preserve embedded NUL bytes. There is no dynamic string allocation,
mutation, concatenation, interpolation or ARC. Small printing, comparison and
checked-arithmetic helpers are emitted with the C; no separate runtime library
is required yet. Integer checks use clang overflow builtins.

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

var point = Vec2 { x: 10.0, y: 20.0 }
point.x = 1.0
print(point.y)
```

Fields are declared one per line, following the statement-boundary rule.
Record construction names every field exactly once: there are no defaults and
no partial initialization, so a missing field is `E0112` and a repeated one is
a duplicate declaration. Field order in construction is free; the backend lays
fields out in declaration order.

A struct cannot contain itself. A value type has no indirection, so the size
would not exist; the checker rejects it rather than the C compiler.

Values copy on assignment, argument passing and return. Assigning to `v.x` or
`v.i.x` requires the binding it is rooted in to be a `var`; a field of a `let`
or of a parameter is immutable.

Struct names live in a type namespace. The value resolver never sees them,
which is why resolution still reports only value names.

`if value { }` and `while value { }` read as a condition followed by a block,
never as record construction. Parentheses make a literal available again, as
does any nested expression context such as a call argument or a field value.

Structs have no methods yet, and classes remain planned below.

## Classes and memory — Planned

The user's [test.skuld](test.skuld) is the living reference for syntax proposals.
The planned class syntax below follows that direction; it is not supported by
the current compiler. These choices replace the earlier explicit `self`
parameter, `func`-prefixed class methods and class record construction.

```skuld
class User {
    name: string
    address: Address

    hello() {
        print("Hello, my name is " + this.name)
    }
}

// Construction syntax; constructor and initialization rules are still open.
// Address must also be declared before this can become a complete example.
var user: User = new User()
```

Class methods use `hello()` or `is_adult() -> bool` without `func` and
without an explicit receiver parameter. `this` denotes the implicit current
instance inside instance methods. Top-level functions retain `func`.
Fields keep explicit type annotations, and method parameters and non-void
returns retain explicit types. Class construction uses `new User(...)`;
`User { ... }` is no longer the planned class construction syntax.

Constructor declarations, constructor arguments, field defaults and definite
initialization rules still need a design before implementation. In particular,
`new User()` does not promise that required fields can remain uninitialized.
The `undefined` output comment in the sketch is not an adopted language value:
Skuld must not expose uninitialized field reads as JavaScript-style undefined.
The sketch's lowercase `address` does not declare a type or establish an alias;
the specification uses `Address` as a placeholder for a separately declared type.

The example's string `+` is planned concatenation, not implemented arithmetic
on strings. String interpolation remains a separate future capability.

Structs are implemented; see the section below. Their method/impl syntax
remains provisional, and the class-method update does not settle struct
receiver rules, so structs currently have fields but no methods.

Structs have value/copy semantics. Classes are managed reference types with
future ARC. No inheritance, garbage collector, borrow checker or Rust ownership
system. Future runtime operations include retain/release and string/array
allocation. No runtime is required for lexical analysis. HIR will eventually
lower methods to calls such as `User_greet(user)`.

## Demonstration proposals — Experimental

`test.skuld` may contain incomplete examples and comments asking for redesign.
Read the current file before syntax work; do not treat it as a passing fixture
or automatically implement every construct it contains.

- Global variable declarations and calls in the sketch demonstrate a possible
  source layout. Executable global statements are still rejected, and
  `func main()` remains the entrypoint. Adopting implicit entrypoints or
  module initialization requires a separate decision; the class example above
  is a syntax sketch, not an exception to the current rule.
- Array literals such as `[1, 4, 6, 7, 3]` remain planned, with the existing
  proposed array type syntax `[]int`.
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
`Option<T>` with `Some`/`None` replaces nullable values; `Result<T, E>` with
`Ok`/`Err` and future `?` propagation represents errors.

Future FFI: `extern "C" { func puts(text: *char) -> int }`.
Future commands: `new`, `fmt`, `test`, `doc`. LLVM/Cranelift and eventual
self-hosting remain long-term possibilities.

## Unsupported features and experimental status

**Experimental:** the demonstration proposals above are documentation-only;
no experimental compiler features are enabled. The full native pipeline, functions, scalar values,
variables and conditional execution (Demos 0–2) are **Implemented**.

Loops, classes, structs, interpolation, arrays and managed-memory runtime
behavior remain **Planned**. No generics, macros, async/await, threads, channels,
reflection, decorators, annotations, package registry, compiler plugins,
compile-time execution, operator overloading or user-defined conversions will
be implemented before Demo 3. Inheritance is excluded from the core design.
