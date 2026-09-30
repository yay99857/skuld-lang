---
title: Syntax
description: The basic spelling and structure of a Skuld source file.
status: Implemented
---
Source files use the `.skuld` extension and UTF-8 text. Identifiers contain ASCII letters, digits and underscores, and cannot begin with a digit.

## Declarations and statements

```skuld main.skuld
func main() {
    let message: string = "Hello"
    var count = 1
    count += 1
    print("${message}: ${count}")
}
```

Keep executable statements inside functions. Module scope holds declarations such as functions, types, constants, statics and imports. There is no semicolon syntax.

## Comments and literals

`//` starts a comment that continues to the end of the line. Block comments are not supported. Strings use double quotes; characters use single quotes.

```skuld
func main() {
    // Integers support decimal, hex, binary and octal.
    let mask = 0xFF
    let million = 1_000_000
    let letter = 'A'
    print("${letter}: ${mask + million}")
}
```

## Statement boundaries

Newlines are not tokens, and expressions can continue across lines. Write each declaration and standalone statement on its own line for clarity. Use the [formatter](/docs/tooling/formatter/) to normalize layout without changing expression boundaries.

## Signatures

Top-level functions use `func`. Methods omit it and use an implicit `this`. Parameters have explicit types. `-> Type` declares the result, and it is the only spelling.
