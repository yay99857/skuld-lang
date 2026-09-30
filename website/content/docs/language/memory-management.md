---
title: Memory management
description: Understand copying, counted references and the lifetime of borrowed pointers.
status: Implemented
---
Skuld uses non-atomic reference counting for managed values. There is no garbage collector, cycle collector or borrow checker.

## Values and references

| Value | Behavior |
| --- | --- |
| Scalars, structs, fixed arrays | Copy on assignment |
| Classes and dynamic arrays | Share referenced storage |
| Strings | Immutable byte sequences; runtime-created storage is counted |
| Option, Result, enums | Inline tag and payload; managed payloads retain/release when active |
| Function values | Non-escaping environments in the enclosing block |

Owning slots release their values when their lexical block exits. Managed fields in copied values preserve their reference semantics.

## Strings and slices

Literals reference static bytes without allocation. Concatenation allocates. A slice of owned string bytes copies; slicing a literal can remain a view because those bytes live for the program's lifetime.

## Weak references

Use `weak Class` for a non-owning class link. `weak(value)` creates it. `upgrade()` returns an owning `Option<Class>` if the target still exists. `alive()` tests liveness without retaining. There is no trapping `get()`; handle the expired case with `if let` or `let ... else`.

```skuld
class User {
    name: string
}

func main() {
    let user = new User(name: "Ada")
    let reference = weak(user)
    if let Some(live) = reference.upgrade() {
        print(live.name)
    }
}
```

Strong cycles require weak links or explicit breaking. A weak reference never keeps the target's managed fields alive. Compiler-created owning temporaries can remain alive until the containing block ends.

## Raw pointers

`ptr(value)` borrows storage and retains nothing. Keep its owner alive for every use of the pointer. Reading or writing through a raw pointer requires an [unsafe block](/docs/advanced/interoperability/#unsafe-pointer-access).

## External resources

Reference counting memory does not automatically close every library resource. Follow API lifetime rules and register `defer resource.close()` immediately after acquiring a resource that needs closing.
