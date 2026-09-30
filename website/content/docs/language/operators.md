---
title: Operators
description: Arithmetic, comparisons, boolean operations and checked bit operations.
status: Implemented
---
Operations require compatible types. Skuld preserves left-to-right evaluation, and boolean operators short-circuit.

## Arithmetic and comparisons

Arithmetic uses `+`, `-`, `*`, `/` and `%`. Integer overflow, division by zero and invalid signed division trap. Strings support concatenation with `+`. Comparisons include `==`, `!=`, `<`, `<=`, `>` and `>=` where the operand types support them.

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
