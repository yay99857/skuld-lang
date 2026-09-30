---
title: Error handling
description: Represent absence with Option and recoverable failure with Result.
status: Implemented
---
Skuld uses values to make expected failures visible. It has no null value and does not use exceptions as its primary error mechanism.

## Optional values

`Option<T>` is either `Some(value)` or `None`. Skuld has no `null`: writing it is an error whose suggested fix is `None`.

```skuld
func main() {
    let value: Option<int> = Some(42)
    if let Some(number) = value {
        print(number)
    } else {
        print("No value")
    }
}
```

The payload is immutable and only exists in the successful branch. `is_some()` and `is_none()` inspect presence. A bare value can wrap into an expected Option.

## Results

`Result<T, E>` holds `Ok(value)` or `Err(error)`. Both constructors need an expected Result type. Bare values do not wrap implicitly.

```skuld
func positive(value: int) -> Result<int, string> {
    if value > 0 {
        return Ok(value)
    }
    return Err("expected a positive integer")
}

func main() {
    match positive(42) {
        Ok(value): print(value)
        Err(error): print(error)
    }
}
```

`if let Ok(value)`, `if let Err(error)`, `is_ok()` and `is_err()` also inspect a Result.

## Propagate with question mark

Postfix `?` yields the success payload or returns the error unchanged. The enclosing function must return a Result with the exact same error type. No error-type conversion happens.

```skuld
func read_number() -> Result<int, string> {
    return Ok(41)
}

func answer() -> Result<int, string> {
    let value = read_number()?
    return Ok(value + 1)
}

func main() {
    match answer() {
        Ok(value): print(value)
        Err(error): print(error)
    }
}
```

## Convert an error at the call

`?` never converts between error types. When a call fails with a different error type than the enclosing function returns, convert it with `map_err`:

```skuld
enum AppError {
    Parse(string)
}

func parse(text: string) -> Result<int, string> {
    if text == "42" {
        return Ok(42)
    }
    return Err("not a number")
}

func run(text: string) -> Result<int, AppError> {
    let value = parse(text).map_err((e) => AppError.Parse(e))?
    return Ok(value + 1)
}

func main() {
    match run("42") {
        Ok(value): print(value)
        Err(_): print("failed")
    }
}
```

`map_err(f)` leaves `Ok` untouched and passes the error through `f`, a non-escaping function value. The new error type is whatever `f` returns. A block-bodied lambda declares it as `(e) -> AppError { ... }`; an expression body infers it. The type is never inferred from the surrounding `?`.

## Unwrap with an escape

`let value = result else error { ... }` extracts a success, with a failure block that must leave the current path. This can keep the success path unindented. An Option can use an escape block without an error binding.

## Traps are not Results

Integer overflow and invalid indexes trap. A trap ends the process and cannot be caught as a Result. [Deferred cleanup](/docs/language/defer/) runs on normal control-flow exits, including propagated errors, but not on traps.
