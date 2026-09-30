---
title: Arrays
description: Use shared dynamic arrays and inline fixed-size arrays.
status: Implemented
---
Dynamic arrays have type `[]T`. Every element has the same type. References share element mutations, including through a `let` binding.

## Dynamic arrays

```skuld
func main() {
    let numbers = [3, 1, 2]
    numbers.push(4)
    numbers.sort((a, b) => a.compare(b))
    for number in numbers {
        print(number)
    }
}
```

Methods include `len()`, `push(value)`, `insert(index, value)`, `pop()`, `remove(index)`, `sort(compare)` and `to_sorted(compare)`. Indexes use `int` and are bounds-checked. An empty literal needs an expected element type, such as `let bytes: []u8 = []`.

## Sorting

`sort()` mutates the shared array and returns void. `to_sorted()` produces a new sorted array. Both are stable. A comparator takes two elements and returns an `Ordering`: `Less`, `Equal` or `Greater`.

```skuld
struct Person {
    name: string
    age: int
}

func main() {
    let people = [
        Person { name: "Bea", age: 30 },
        Person { name: "Al", age: 30 },
    ]
    let sorted = people.to_sorted((a, b) => a.age.compare(b.age).then(a.name.compare(b.name)))
    print(sorted[0].name)
}
```

`compare` exists on every integer width, `char`, `bool` and `string`. Strings compare bytewise, with no locale. `float` offers `total_compare`, which follows IEEE 754 totalOrder. `then` breaks a tie with a second key, and swapping the arguments reverses an order.

> [!Note]
> A comparator that returns `int`, such as `(a, b) => a - b`, is a type error. Subtraction traps when the difference overflows, so `compare` replaces it.

## Slices

`values[start..end]` copies a half-open range into an owned dynamic array. Managed elements are retained and released.

## Fixed-size arrays

`[N]T` is an inline value with a positive constant length. Use a repeated literal such as `var buffer: [16]u8 = [0; 16]` or a list checked against an expected fixed-array type. Assignment copies the array.

A fixed array can widen to a read-only `[]T` view without allocation. It must not escape its storage lifetime; visible escapes are rejected and mutation through the view traps. Slice it when an owned dynamic copy is needed.
