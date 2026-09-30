---
title: Introduction
description: Meet Skuld, an experimental statically typed language that compiles to native code.
status: Implemented
---
Skuld is an experimental, statically typed programming language designed for clear, predictable software. It combines explicit function signatures with local type inference and compiles to native executables through C and clang.

## A small first look

Functions use `func`. Values bound with `let` cannot be reassigned; `var` bindings can. Types are checked before a program runs.

```skuld main.skuld numbers highlight=6
func greet(name: string) -> string {
    return "Hello, ${name}!"
}

func main() {
    let message = greet("Skuld")
    print(message)
}
```

## What Skuld provides

- **Strong static typing.** No implicit numeric conversions or truthiness.
- **Native execution.** The compiler emits C, then clang builds an executable.
- **Explicit data models.** Structs copy values; classes share references.
- **Visible error handling.** `Option<T>` represents absence and `Result<T, E>` represents success or failure.
- **Integrated tools.** Check, build, run, format and test source files, with a language server for editors.

## Current status

> [!Experimental]
> Skuld is under active development. “Implemented” means a feature exists in the current compiler, not that its syntax or API is stable. These pages document the current source checkout under version 0.1.0.

Linux x86_64 and Windows x86_64 are supported. i686 Linux is tested as a second native target. macOS is unverified. HTTPS needs OpenSSL installed on the machine, and freestanding builds have their own restrictions.

General generics, threads, async/await and a package registry are not implemented. The builtins `Option` and `Result` do not imply support for user-defined generic types.

## Where to go next

Start with [installation](/docs/getting-started/installation/), then write [Hello World](/docs/getting-started/hello-world/). If you already have the compiler, explore [functions](/docs/language/functions/) or the [standard library](/docs/standard-library/overview/).
