# Runtime

`strings.c` is the managed-memory runtime. It is embedded verbatim into every
generated program, so this file is the single source for retain and release and
the code generator never restates them.

## Decisions

**Reference counting, not garbage collection.** A value carrying references is
released when its last strong reference goes away. Weak references can keep
allocation storage alive after managed fields have been destroyed.

**Counts are not atomic.** Skuld has no threads, so atomic operations on every
copy would cost without buying anything. Adding threads means revisiting this
file, not the code generator.

**No cycle collector.** Strings cannot form cycles, but classes and arrays can.
Use weak class back-links or explicitly break strong cycles. Weak references do
not collect a graph automatically.

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

## Classes, arrays and weak references

Classes and arrays start with a `skuld_object` header. The runtime owns strong
and weak count operations. Generated helpers only describe the field or element
destructor and adapt the concrete type to the runtime. Forward helper declarations
support mutually referring classes and managed types declared later in source.

A live object has one implicit weak count in addition to explicit weak handles.
The last strong release invokes its destructor, then drops that implicit weak
count. This prevents a self-weak field from freeing the header during destruction.
Explicit weak handles keep the allocation storage, but not its managed fields,
alive. The last weak release frees storage. `get()` checks liveness before
retaining, so an expired target cannot be accessed. Counts are non-atomic.

Arrays use a checked allocation size and a fixed length with typed element
storage. Their destructor releases each managed element. Indexes are checked
before reads or writes. Strong cycles can still leak; this is intentional until
the program breaks them or uses weak class links.

Reference receivers are retained in temporaries before an assignment RHS runs.
The selected object or array remains alive if the RHS replaces another reference.
Owning temporaries currently last until the containing lexical block exits,
which can delay weak expiration beyond the last source-level use.

## Verification

`cli/tests/native.rs` builds every `tests/pass` fixture with address, leak and
undefined-behaviour detection. A leak or a double free fails the suite.
