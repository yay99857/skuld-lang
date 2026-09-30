---
title: Control flow
description: Branch with booleans, iterate over ranges and arrays, and match values.
status: Implemented
---
Conditions require `bool`. Braces delimit blocks; parentheses around conditions are optional.

## If and else

```skuld
func main() {
    let age = 20
    if age >= 18 {
        print("Adult")
    } else {
        print("Minor")
    }
}
```

## For loops

`for` iterates over a half-open integer range or an array. Range bounds are evaluated once, and the loop variable is immutable.

```skuld
func main() {
    for i in 0..3 {
        print(i)
    }
    for word in ["clear", "predictable"] {
        print(word)
    }
}
```

The range prints `0`, `1`, `2`. If its start is at least its end, the body does not run.

## While and loop

`while condition` checks its condition before each iteration. `loop` repeats until control leaves it. `break` exits the nearest loop; `continue` advances to its next iteration.

## Match

`match` dispatches by enum variant, Result case, scalar value or string. Enum matches must cover every variant or use `_`. Scalar and string matches always need a wildcard. See [enums](/docs/language/enums/) and [error handling](/docs/language/error-handling/).
