---
title: Variables
description: Choose immutable bindings, mutable bindings and compile-time constants.
status: Implemented
---
Use `let` by default. Use `var` when the binding needs to change. Every local binding requires an initializer.

## Let and var

```skuld main.skuld
func main() {
    let limit = 10
    var count: int = 0
    count += 1
    print(count < limit)
}
```

Local types can be inferred from their initializer. An annotation checks the value against that type. Parameters and loop variables are immutable.

## Bindings and referenced objects

An immutable binding cannot be reassigned. A class or dynamic array referenced by that binding can still be mutated. Struct value fields require a mutable binding.

```skuld
func main() {
    let numbers = [1, 2]
    numbers.push(3)
    print(numbers.len())
}
```

## Constants and statics

`const` evaluates at compile time. Constants can be declared at module or function scope; public module constants use `pub const`.

```skuld
const LIMIT: int = 16
static counter: int = 0

func main() {
    counter += 1
    print(LIMIT + counter)
}
```

Module-level `static` storage holds a scalar or a zero-initialized fixed array of scalars. Its initializer must be a constant expression. Managed values cannot be statics.

## Scope and shadowing

A child scope can shadow an outer name. A local becomes visible after its initializer. Duplicate declarations in the same scope are errors.
