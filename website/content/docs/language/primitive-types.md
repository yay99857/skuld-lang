---
title: Primitive types
description: Understand scalar widths, characters, strings and explicit conversions.
status: Implemented
---
Skuld checks types statically. It does not coerce integers, floats or booleans implicitly.

## Scalar types

| Type | Meaning |
| --- | --- |
| `int`, `i64` | Two spellings of the same signed 64-bit integer |
| `i8`, `i16`, `i32` | Signed integers of the named width |
| `u8`, `u16`, `u32`, `u64` | Unsigned integers of the named width |
| `isize`, `usize` | Distinct signed and unsigned target pointer-width integers |
| `float` | IEEE binary64 floating point |
| `bool` | `true` or `false` |
| `char` | A Unicode scalar value, not a grapheme |
| `void` | No return value; not a local or parameter type |

## Explicit conversions

```skuld
func main() {
    let byte: u8 = 200
    let wide = int(byte)
    let truncated = int(3.7)
    let letter = char(65)
    print("${wide}, ${truncated}, ${letter}")
}
```

Conversions trap when the result cannot fit. Float-to-integer conversion truncates toward zero and rejects out-of-range values and NaN. Converting to `char` checks for a valid Unicode scalar.

## Integer literals

A literal takes its expected integer width, or defaults to `int`. Widths never mix implicitly. The signed minimum is written with a minus on the literal, such as `-128` for an `i8`.

## Strings and bytes

`string` is an immutable, length-aware byte sequence. `len()` counts bytes, indexing returns `u8`, and embedded NUL bytes are preserved. A string is not automatically NUL-terminated. Use [UTF-8 helpers](/docs/standard-library/strings/#unicode-and-c-strings) when interpreting characters.

There is no null value. Absence uses [Option](/docs/language/error-handling/#optional-values).
