---
title: Strings & text
description: Search, split and decode length-aware byte strings.
status: Implemented
---
Import `"std/strings"` for text helpers. All offsets and builtin string lengths count bytes, not Unicode characters.

## Builtin string operations

| Operation | Result |
| --- | --- |
| `text.len()` | Byte length as `int` |
| `text[index]` | One `u8`, bounds-checked |
| `text[start..end]` | Half-open byte slice |
| `text.bytes()` | An array of bytes |

## Search and trim

```skuld
func starts_with(text: string, prefix: string) -> bool
func ends_with(text: string, suffix: string) -> bool
func index_of(text: string, needle: string) -> Option<int>
func contains(text: string, needle: string) -> bool
func trim(text: string) -> string
```

These are signatures from `std/strings`, not a complete executable program. `index_of` returns the first byte offset; an empty needle is found at zero. `trim` removes ASCII space, tab, carriage return and newline from both ends.

| Parameter | Type | Meaning |
| --- | --- | --- |
| `text` | `string` | Input byte sequence |
| `needle` | `string` | Bytes to locate |
| `prefix` / `suffix` | `string` | Bytes compared at the corresponding end |

## Split and join

```skuld
func split(text: string, separator: string) -> []string
func join(parts: []string, separator: string) -> string
```

Splitting keeps empty fields. An empty separator returns the whole input as one field.

```skuld main.skuld
import "std/strings"

func main() {
    let parts = strings.split("red,green,blue", ",")
    print(strings.join(parts, " / "))
}
```

## Unicode and C strings

`std/utf8.validate(bytes)` validates UTF-8; `decode(bytes)` returns a string; `count(bytes)` counts Unicode scalar values. These operations return Results with `Utf8Error`. `std/cstring.to_c(text)` returns a NUL-terminated `[]u8` or `CStringError` and rejects embedded NUL bytes.

## Availability

Implemented in the current 0.1.0 source checkout. String bytes are immutable. Unicode grapheme segmentation and locale-aware text processing are not provided by these APIs.
