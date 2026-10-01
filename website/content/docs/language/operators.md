---
title: Operators
description: Arithmetic, comparisons, boolean operations and checked bit operations.
status: Implemented
---
Operations require compatible types. Skuld preserves left-to-right evaluation, and boolean operators short-circuit.

## Arithmetic and comparisons

Arithmetic uses `+`, `-`, `*`, `/` and `%`. Integer overflow, division by zero and invalid signed division trap. Strings support concatenation with `+`. Comparisons include `==`, `!=`, `<`, `<=`, `>` and `>=` where the operand types support them.

## Equality

`==` and `!=` compare two values of one type and convert nothing, so `opt == 5` is an error and `opt == Some(5)` is the spelling. They compare scalars, strings by their bytes, value structs field by field, enums, fixed arrays, `Option` and `Result`, whenever every part of the type can be compared. Floats follow IEEE 754, so an aggregate holding NaN is not equal to itself.

```skuld
struct Point {
    x: int
    y: int
}

func main() {
    let a = Point { x: 1, y: 2 }
    print(a == Point { x: 1, y: 2 })
    let maybe: Option<int> = 5
    print(maybe == Some(5))
    print(maybe.is_none())
}
```

Comparing with a variant that has no payload, as in `status == .Pending`, tests only the tag, so it works on any enum. Classes, dynamic arrays, interfaces, weak references, function values, raw pointers, extern layouts and enums with an `indirect` variant cannot be compared structurally, and the error names the field that prevents it. Absence is tested with `is_none()`, not `== None`.

## Boolean operations

Use `&&`, `||` and `!` with `bool` values. There is no truthiness: an integer or string cannot stand in for a condition. The right operand of `&&` or `||` is only evaluated when needed.

## Bit operations

Integers support `&`, `|`, `^`, `~`, `<<` and `>>`. Signed right shift is arithmetic; unsigned right shift is logical. A shift amount below zero or at least the integer width traps.

```skuld
func main() {
    var flags: u8 = 0b0000_0011
    flags |= 0b0000_0100
    print(int(flags))
}
```

## Assignment

Compound forms include `+=`, `-=`, `*=`, `/=`, `%=`, `&=`, `|=`, `^=`, `<<=` and `>>=`. A compound assignment snapshots the old value before evaluating its right-hand side.

## Grouping

Use parentheses when combining operator families. Bitwise operators bind more tightly than comparisons, so `flags & mask == mask` compares the masked result. Parentheses can make that contract explicit: `(flags & mask) == mask`.
