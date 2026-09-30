---
title: Functions
description: Declare typed functions, pass arguments and use non-escaping function values.
status: Implemented
---
Functions use the `func` keyword. Parameters require explicit types, and a function returning a value declares its return type.

## Declaring functions

```skuld main.skuld numbers highlight=1,6
func add(a: int, b: int) -> int {
    return a + b
}

func main() {
    print(add(20, 22))
}
```

This program prints `42`. Functions are predeclared, so a call can appear before the function's declaration.

## Parameters

Arguments are evaluated left to right. Their types must match the signature; numeric widths do not mix implicitly. Parameters are immutable bindings. Copy one into a local `var` if you need to reassign it.

## Return values

Omitting a return type means `void`. A non-void function must return on every path or diverge. Use `return value` to produce the result and bare `return` to leave a void function.

> [!Tip]
> A return type is always written `-> Type`, lambdas included. `: Type` in that position is a syntax error whose suggested fix is the arrow.

## Function values and lambdas

A function type is written `(int) -> int`. Lambdas use a block body or an expression body with `=>`. An expected function type can supply missing annotations.

```skuld
func apply(value: int, transform: (int) -> int) -> int {
    return transform(value)
}

func main() {
    let base = 10
    print(apply(5, (n) => n + base))
}
```

Lambdas capture immutable bindings by value. A mutable `var` cannot be captured.

## Non-escaping values

Function values can be locals or parameters. They cannot be fields, return values, array elements or enum, Option or Result payloads. Use a [class interface](/docs/language/interfaces/) for an object with behavior that must be stored.

## Methods

Methods omit `func` and receive an implicit `this`. Read [structs](/docs/language/structs/) for value receivers and [classes](/docs/language/classes/) for shared objects.
