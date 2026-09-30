---
title: Freestanding builds
description: Compile the unmanaged language subset without a hosted runtime or libc.
status: Implemented
---
Freestanding mode is the same language with a different runtime boundary. The checker rejects values that need managed runtime support. The current supported scope is Linux.

## Build commands

```bash
skuld check --freestanding kernel.skuld
skuld emit-c --freestanding kernel.skuld
skuld build --freestanding kernel.skuld -o kernel.o
```

A freestanding build produces an object file, not a linked executable. Startup code, linking and target integration are the caller's responsibility. No linker arguments are accepted.

## Supported values

Scalars, fixed arrays, unmanaged structs, enums, unions, pointers, constants and statics remain available. Managed values such as strings, dynamic arrays, classes, interfaces and weak references are refused, including through containing types. `print` is unavailable.

## Entry and traps

No `main` is generated or required. Public functions are emitted with their source names for external startup code to call. Private functions retain generated names.

Bounds checks and checked arithmetic remain. A trap uses `__builtin_trap()` because there is no standard error stream or runtime unwinder.
