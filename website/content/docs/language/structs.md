---
title: Structs
description: Group fields into a value type with explicit construction and copy semantics.
status: Implemented
---
A struct is a value type. Assignment, argument passing and returns copy its fields. Referenced classes and arrays inside a struct retain their sharing behavior.

## Declare and construct

```skuld
struct Point {
    x: int = 0
    y: int = 0
}

func main() {
    var point = Point { x: 3, y: 4 }
    let copy = point
    point.x = 10
    print(copy.x)
}
```

The program prints `3`. Construction names every field without a default. A struct whose fields all have defaults can use `Point {}`.

## Field mutation

Writing a value field requires a mutable place rooted in a `var`. The fields of a struct bound with `let` are immutable. A referenced class or array within a struct can still be mutated.

## Methods

Methods omit `func` and use `this`. By default the receiver is an immutable copy, so a method cannot change its caller's value fields.

```skuld
struct Rectangle {
    width: int
    height: int
    area() -> int {
        return this.width * this.height
    }
}

func main() {
    print(Rectangle { width: 6, height: 7 }.area())
}
```

## Methods that change the struct

A method declared `var` receives the caller's own storage, so it can change `this`:

```skuld
struct Counter {
    count: int = 0

    var increment() {
        this.count += 1
    }
}

func main() {
    var counter = Counter {}
    counter.increment()
    counter.increment()
    print(counter.count)
}
```

The receiver must be a `var` local, or `this` inside another `var` method, possibly reached through struct fields. An array element or a class field is refused, because the method could move or free the storage it is changing. Copy the value into a `var`, call the method, then write it back:

```skuld
var item = points[i]
item.translate(1, 0)
points[i] = item
```

Class methods cannot be `var`: a class's `this` is already a shared reference.

## Layout

Ordinary struct layout is deliberately unspecified. Use [extern layouts](/docs/advanced/interoperability/#layout-and-the-abi) when another system owns the memory representation.
