---
title: Generics
description: Understand the boundary between builtin parameterized types and future user-defined generics.
status: Planned
---
General user-defined generics are not implemented. There is no supported `func identity<T>` syntax or generic class declaration.

## What works today

`Option<T>` and `Result<T, E>` are compiler builtins. Array element types are also supported by the compiler. These forms do not expose a general type-parameter mechanism to user code.

## What remains undecided

Constraints, representation and the relationship between builtin and user-defined parameterized types require language design work. This page intentionally provides no proposed syntax as executable code.

## Current alternatives

Use a concrete type, a [class interface](/docs/language/interfaces/) for shared behavior, or a [function parameter](/docs/language/functions/#function-values-and-lambdas) for a non-escaping operation. The standard library's string-keyed map is concrete rather than generic.
