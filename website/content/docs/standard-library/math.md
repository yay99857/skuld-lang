---
title: Math
description: Current numeric primitives and the status of a dedicated math module.
status: Planned
---
There is no `std/math` module in the current source tree. Do not import it or assume a set of transcendental functions is available.

## Implemented numeric operations

Arithmetic, comparisons, bit operations and explicit integer/float conversions are language features. See [operators](/docs/language/operators/) and [primitive types](/docs/language/primitive-types/).

Integer arithmetic is checked. Floating-point arithmetic uses binary64 without fast-math; infinities and NaNs can result from runtime operations.

## Foreign libraries

A program can declare a compatible C function through the [foreign interface](/docs/advanced/interoperability/) and link its library explicitly. The declaration's correctness is the author's responsibility.

## Future scope

A standard math module and its API are not specified here. This page records the gap rather than promising names or availability.
