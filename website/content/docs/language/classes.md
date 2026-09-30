---
title: Classes
description: Model shared mutable objects with reference-counted class instances.
status: Implemented
---
A class has reference semantics. Multiple bindings can refer to the same object, and the runtime counts owning references.

## Declare a class

```skuld main.skuld
class User {
    name: string = "Guest"

    hello() {
        print("Hello, ${this.name}!")
    }
}

func main() {
    let user = new User(name: "Ada")
    let alias = user
    alias.name = "Grace"
    user.hello()
}
```

This prints `Hello, Grace!`. An immutable binding prevents reassignment of the reference; it does not freeze the object's fields.

## Construction and defaults

Use `new Class(field: value)`. Supply every field without a default. If all fields have defaults, `new User()` is valid.

Explicit arguments run left to right. Remaining field defaults then run in declaration order, once per construction. Defaults cannot access `this` or sibling fields.

There is no constructor body or partially initialized object. Initialization that can fail belongs in a function returning `Result`.

## Methods and composition

Methods omit `func`, have an implicit `this`, and may mutate fields through that reference. Classes can implement [interfaces](/docs/language/interfaces/). Inheritance is not supported; compose objects using fields.

## Ownership cycles

Strong cycles are not collected. Use [weak references](/docs/language/memory-management/#weak-references) for back-links or explicitly break a cycle when it is no longer needed.
