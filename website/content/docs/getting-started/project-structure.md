---
title: Project structure
description: Organize a program around its entry file and directory-based modules.
status: Implemented
---
A Skuld program has no required project manifest. Its root is the directory containing the entry file passed to the compiler.

## A small project

```text
my-program/
  main.skuld
  geometry/
    point.skuld
```

The files in an imported module directory share a namespace. Imported declarations must be explicitly public.

## Export a type

```skuld geometry/point.skuld
pub struct Point {
    x: int
    y: int
}
```

## Use the module

```skuld main.skuld
import "geometry"

func main() {
    let point = geometry.Point { x: 3, y: 4 }
    print(point.x)
}
```

```bash
skuld run main.skuld
```

Imports resolve from the entry directory, not the importing file. Paths cannot escape the program root. Read the [module rules](/docs/language/modules/) before splitting a larger program.
