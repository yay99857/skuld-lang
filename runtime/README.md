# Runtime

`strings.c` is the managed-memory runtime. It is embedded verbatim into every
generated program, so this file is the single source for retain and release and
the code generator never restates them.

## Decisions

**Reference counting, not garbage collection.** A value carrying references is
freed when the last one goes away, at a point the programmer can predict.

**Counts are not atomic.** Skuld has no threads, so atomic operations on every
copy would cost without buying anything. Adding threads means revisiting this
file, not the code generator.

**No cycle collector.** Strings are immutable and cannot form cycles, so
nothing here can leak. Aggregates that can form cycles will need `weak`
references when they arrive; that is a language feature, not a collector.

**Literals do not allocate.** A string's `owner` is null when its bytes are
static, which makes retain and release no-ops for literals.

## How ownership is emitted

Every owning slot — a local, a temporary, a struct that holds one — is declared
with `__attribute__((cleanup(...)))`, so it is released on every exit path,
including `return`, `break` and `continue`, which C cannot otherwise express.

A value that already carries its own reference, such as a fresh concatenation
or a function result, is adopted as-is. A borrowed value, such as reading a
variable or a field, is retained as it enters a slot. Arguments are borrowed:
the caller's slot outlives the call. A returned value is retained before the
return, because every local is released as the function exits.

Structs that hold references get generated `retain`, `release` and `assign`
functions that walk their managed fields; unmanaged types get none and cost
exactly what they did before.

## Verification

`cli/tests/native.rs` builds every `tests/pass` fixture with address, leak and
undefined-behaviour detection. A leak or a double free fails the suite.
