---
title: Modules
description: Import directory-based modules with qualified public names.
status: Implemented
---
A module is a directory whose `.skuld` files share a namespace. Imports are rooted at the entry file's directory. The reserved `std` prefix selects sources embedded in the compiler.

## Import a module

```skuld
import "std/strings"

func main() {
    print(strings.trim("  Skuld  "))
}
```

The final path segment becomes the qualifier: `std/strings` is used as `strings`. Imported names are always qualified.

## Export declarations

Mark exported declarations with `pub`. A declaration without it stays inside its module. See [visibility](/docs/language/visibility/) for types and methods.

## Paths and cycles

Paths cannot climb outside the entry directory. Import cycles are diagnostics, not a supported initialization order. A directory named `std` beside the program cannot replace the embedded standard library.

## Project boundaries

There is no package manifest or package registry. A program's source tree and its entry file establish its module graph. See the [project example](/docs/getting-started/project-structure/).
