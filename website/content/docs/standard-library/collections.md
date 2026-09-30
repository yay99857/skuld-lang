---
title: Collections
description: Combine builtin arrays with the library's string-keyed integer map.
status: Implemented
---
Use [arrays](/docs/language/arrays/) for homogeneous sequences. The standard library also provides a concrete string-to-integer map through `std/map`.

## StringMap

```skuld main.skuld
import "std/map"

func main() {
    let counts = map.new_map()
    counts.set("visits", 42)
    if let Some(value) = counts.get("visits") {
        print(value)
    }
}
```

`new_map() -> StringMap` creates an empty map. `set(key: string, value: int)` inserts or updates a value. `get(key: string) -> Option<int>` returns absence for a missing key, `has(key: string) -> bool` tests membership, and `len() -> int` counts living entries.

## Representation and order

The map uses open addressing and preserves insertion order for iteration. It is intended as an index: values can be positions, counts or identifiers while the actual objects live in another array.

## Limits and availability

Implemented in the current 0.1.0 source checkout. It is not `Map<K, V>` and does not imply support for general generics. Values are `int`, and keys are `string`.
