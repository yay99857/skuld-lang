---
title: Generics
description: Write one function or type for many element types, with type parameters checked once and compiled per use.
status: Implemented
---
Functions, structs, classes and enums can take type parameters. A generic body is checked once, where it is written, and every combination of type arguments the program uses becomes its own compiled function or type, so a generic costs what a hand-written copy would.

## Generic functions

```skuld
func first<T>(items: []T) -> Option<T> {
    if items.len() == 0 {
        return None
    }
    return items[0]
}

func map<T, U>(items: []T, f: (T) -> U) -> []U {
    let out: []U = []
    for item in items {
        out.push(f(item))
    }
    return out
}

func main() {
    let numbers: []int = [3, 1, 2]
    if let n = first(numbers) {
        print(n)
    }
    let words: []string = ["alpha", "beta"]
    let lengths = map(words, (word) => word.len())
    print(lengths[1])
}
```

Type arguments are never written at a call: they are inferred. An argument with a type of its own fixes what it matches, the type the call is expected to have fills in the rest, and arguments that take their type from context — an integer literal, `None`, a lambda — are checked last. `let small: Option<u8> = first([200, 7])` makes the literals `u8`. When nothing fixes a parameter, the error asks for an annotated `let`.

## Generic types

```skuld
struct Pair<A, B> {
    first: A
    second: B
}

class Stack<T> {
    items: []T = []

    push(item: T) {
        this.items.push(item)
    }

    pop() -> Option<T> {
        return this.items.pop()
    }
}

enum Tree<T> {
    Leaf
    indirect Node(Branch<T>)
}

struct Branch<T> {
    left: Tree<T>
    value: T
    right: Tree<T>
}

func main() {
    let stack = new Stack<string>()
    stack.push("one")
    let pair = Pair { first: 1, second: "x" }
    let tree: Tree<int> = .Leaf
    print(pair.second)
}
```

A type with parameters is always written with its arguments in a type: `Stack<string>`, `weak Stack<Dog>`. After `new` they are written too, or inferred from the context or the fields. Methods use the parameters of their type and declare none of their own.

## What a type parameter can do

Inside a generic body a `T` is opaque. A value of it can be stored, passed, returned and captured, and nothing else: there is no `==`, no `print`, no method and no conversion on `T`, because one body serves every instance. Pass the operation in as a function value instead:

```skuld
func index_where<T>(items: []T, matches: (T) -> bool) -> Option<int> {
    for i in 0..items.len() {
        if matches(items[i]) {
            return i
        }
    }
    return None
}

func main() {
    let words: []string = ["alpha", "beta"]
    let found = index_where(words, (word) => word == "beta")
    print(found.is_some())
}
```

Outside generic code an instance is an ordinary concrete type, so two `Pair<int, int>` values compare with `==` as any struct does.

## Not generic

`main`, test functions, `extern struct`, numbered enums and interfaces cannot be generic, and neither can a `pub func` under `--freestanding`. Constraints such as "comparable", and with them a `Map<K, V>` with keys other than strings, are a later decision. `Option<T>` and `Result<T, E>` stay builtin: they are spelled and instantiated like any generic type, and keep their own sugar — the implicit wrap, `None`, `?` and `if let`.
