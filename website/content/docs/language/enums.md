---
title: Enums
description: Represent alternatives with sum types and exhaustive pattern matching.
status: Implemented
---
An enum contains named variants. A variant can be a unit case or hold one payload.

## Variants and matching

```skuld
enum Status {
    Pending
    Ready(int)
}

func show(status: Status) {
    match status {
        Status.Pending: print("Waiting")
        Status.Ready(value): print(value)
    }
}

func main() {
    show(Status.Ready(42))
}
```

Payload bindings are immutable and scoped to their match arm. Cover every variant or include a wildcard `_`. A pattern can leave the enum name out, as in `Pending:` and `Ready(value):`, because the value being matched already says which enum it is. `:` is the only separator between a pattern and its statement.

## Implicit variants

Where the context expects an enum, write a variant with a leading dot and leave the enum name out:

```skuld
enum Status {
    Pending
    Ready(int)
}

func show(status: Status) {
    match status {
        Pending: print("Waiting")
        Ready(value): print(value)
    }
}

func main() {
    show(.Ready(42))
    var status: Status = .Pending
    show(status)
}
```

This works for arguments, returns, annotated bindings, assignments, fields and array elements. Under an expected `Option<Status>`, `.Ready(1)` wraps automatically. Without an expected enum type, `.Name` is an error. Enums have no `==`; use `match` to test a variant.

## Value and range patterns

Scalars and strings also support matching. Literal and constant patterns are accepted, as are `a..b` and `a..=b` ranges. These matches require `_` even if the written cases appear to cover every value.

## Numbered enums

`enum Protocol: u8 { Tcp = 6 }` assigns an integer representation to unit variants. Duplicate values and payload variants are refused. Convert to the number with `u8(value)`; convert back with `Protocol(number)`, which traps if no variant matches.

## Recursive enums

A variant marked `indirect` stores its payload in a reference-counted box, so the enum can contain itself:

```skuld
enum List {
    Empty
    indirect Cons(Cell)
}

struct Cell {
    head: int
    tail: List
}

func sum(list: List) -> int {
    match list {
        Empty: return 0
        Cons(cell): return cell.head + sum(cell.tail)
    }
}

func main() {
    let list = List.Cons(Cell { head: 1, tail: .Cons(Cell { head: 2, tail: .Empty }) })
    print(sum(list))
}
```

Copies share the box. This is safe because a payload is never assigned in place. You have to write `indirect` yourself; the compiler never infers it, because boxing makes the enum a managed value. Using `indirect` on a variant without a payload, or on one that never recurses, is an error. Enums with an `indirect` variant are unavailable in freestanding builds and statics.

> [!Warning]
> Releasing a chain recurses once per link, just like a chain of classes does. On the default Windows stack, a 10,000-link list releases and a 100,000-link list overflows.

## Managed payloads

Enum values use inline tag/payload semantics. A managed payload is retained and released according to its active variant. Builtin [Result](/docs/language/error-handling/) uses the same approach.
