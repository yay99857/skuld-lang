---
title: Defer
description: Register cleanup at the lexical scope that owns a resource.
status: Implemented
---
`defer statement` runs its statement when the enclosing block is left. Multiple deferred statements run in reverse registration order.

## Run at scope exit

```skuld
func main() {
    var count = 0
    defer print(count)
    count = 42
}
```

This prints `42`: deferred statements read the bindings' values at exit, rather than capturing their argument values at registration.

## Exit paths

Cleanup runs on fallthrough, `return`, `break`, `continue`, `?` propagation and a `let ... else` escape. In a loop body it belongs to that iteration. The return value is evaluated before deferred statements run.

> [!Important]
> A trap ends the process. Deferred statements do not run on traps.

## Restrictions

A deferred statement cannot declare a binding or leave itself using `return`, `break`, `continue` or `?`. There is no `errdefer`. When cleanup depends on whether ownership was handed off, use an explicit flag read by the deferred statement.

## Resource cleanup

See [filesystem streaming](/docs/standard-library/filesystem/#streaming-files) for `defer file.close()`. Cleanup belongs to a scope; user-defined destructors are not supported.
