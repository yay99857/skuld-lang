---
title: JSON
description: Parse JSON into a typed value, look up object members and render it again.
status: Implemented
---
Import `"std/json"`. The parser is written in Skuld rather than implemented as a special compiler feature.

## Parse and render

```skuld
func parse(text: string) -> Result<JsonValue, JsonError>
func render(value: JsonValue) -> string
func lookup(value: JsonValue, key: string) -> Option<JsonValue>
```

`parse` returns a structured value or an error. `render` serializes a value. `lookup` searches an object member without introducing a null value into the language.

```skuld main.skuld
import "std/json"

func main() {
    match json.parse("{\"language\":\"Skuld\"}") {
        Ok(value): print(json.render(value))
        Err(error): print(json.describe(error))
    }
}
```

## Data model

`JsonValue` is a sum type. Objects retain their source member ordering and preserve duplicate keys in their representation. Inspect variants with `match` and optional lookups with `if let`.

## Availability

Implemented in the current 0.1.0 source checkout. The library works on strings and byte parsing primitives; it needs no external JSON package.
