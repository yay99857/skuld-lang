---
title: Visibility
description: Keep implementation details local and export deliberate module APIs.
status: Implemented
---
Declarations are private to their module unless marked `pub`. Imported names remain qualified by the module's final path segment.

## Public declarations

```skuld geometry/point.skuld
pub struct Point {
    x: int
    y: int
}

pub func origin() -> Point {
    return Point { x: 0, y: 0 }
}
```

Callers use `geometry.origin()` after importing `"geometry"`. Public constants and statics use `pub const` and `pub static`.

## Members

Methods are public with the type that owns them. Skuld does not provide a separate private-method or protected-member system. Exported type signatures must not expose private types that the importer cannot name.

## Local scope

Block scope is independent of module visibility. Local bindings become visible after their initializer, and child scopes may shadow them. `pub` is not a way to export a local binding.
