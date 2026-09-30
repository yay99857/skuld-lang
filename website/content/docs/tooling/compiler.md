---
title: Compiler
description: Check Skuld source and compile it to a native executable.
status: Implemented
---
The toolchain checks a whole program before generating C. clang then builds the generated code into a native executable.

## Static checking

```bash
skuld check main.skuld
```

Success is silent with exit code 0. Compiler diagnostics use exit code 1 and include a source location. Checking does not invoke clang.

## Run and build

```bash
skuld run main.skuld
skuld build main.skuld -o application
```

`run` uses temporary output and executes it. `build` keeps the executable. Source is never overwritten by a build. Program input and output are inherited by `run`.

## Inspect generated C

```bash
skuld emit-c main.skuld
```

This command performs all checking and emits the backend's C output. `lex`, `parse` and `resolve` inspect earlier stages and are not full type checks.

## Native dependencies

Use `-l<library>` and `-L<directory>` for linked libraries. Arbitrary linker flags are not accepted. [Freestanding mode](/docs/advanced/freestanding/) produces an object file and accepts no linker arguments.
