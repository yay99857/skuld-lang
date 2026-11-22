---
title: Generics
description: Understand the boundary between builtin parameterized types and future user-defined generics.
status: Planned
---
General user-defined generics are not implemented. There is no supported `func identity<T>` syntax or generic class declaration.

## What works today

`Option<T>` and `Result<T, E>` are compiler builtins. Array element types are also supported by the compiler. These forms do not expose a general type-parameter mechanism to user code.

## What remains undecided

A design for unconstrained type parameters on functions, structs, classes and enums is recorded in the roadmap as M36 and is not implemented, so nothing on this page compiles yet. Constraints, and with them a general `Map<K, V>`, are a later decision.

## Current alternatives

Use a concrete type, a [class interface](/docs/language/interfaces/) for shared behavior, or a [function parameter](/docs/language/functions/#function-values-and-lambdas) for a non-escaping operation. The standard library's string-keyed map is concrete rather than generic.
