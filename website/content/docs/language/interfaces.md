---
title: Interfaces
description: Declare a shared method contract for class instances.
status: Implemented
---
An interface names method signatures. A class explicitly declares conformance; matching methods alone do not imply it.

## Declare conformance

```skuld
interface Printable {
    text() -> string
}

class Label: Printable {
    value: string
    text() -> string {
        return this.value
    }
}

func show(item: Printable) {
    print(item.text())
}

func main() {
    show(new Label(value: "Skuld"))
}
```

The checker verifies each method's signature. Methods use the same implicit `this` model as a class method.

## Storing behavior

An interface value contains a counted object reference and its method table. It can be stored for later use, unlike a non-escaping function value.

## Current limits

Only classes implement interfaces. Struct conformance, interface inheritance, default method bodies and runtime downcasts are not supported. See [function values](/docs/language/functions/#non-escaping-values) for short-lived callbacks.
