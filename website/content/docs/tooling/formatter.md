---
title: Formatter
description: Apply the official style or check formatting without modifying source.
status: Implemented
---
The official formatter preserves line comments, literal representations and program semantics.

## Format a file

```bash
skuld fmt main.skuld
```

This updates the source file in place. The formatter requires source that parses successfully.

## Check in automation

```bash
skuld fmt --check main.skuld
```

Check mode does not write. Formatting drift produces exit code 1, making it suitable for a CI check.

## Line width

Lines are kept to 100 columns where the formatter can. A call's arguments, the fields of `new C(...)` and `S { ... }`, an array literal and the parameters of a function, method or lambda go one per line when they do not fit, each followed by a comma:

```skuld
let request = new http.Request(
    url: "https://api.github.com/repos/owner/repository/issues",
    headers: [agent, version],
)
```

A list that fits is put back on one line, however it was written, so the result depends only on the program. A list with a `//` comment inside it is always broken, and each comment stays with the element it was written beside. Long strings, operator chains and match arms are left as they are, so a line can still be longer than 100 columns.

## Normalized syntax

Return types are written `-> Type`, the only accepted spelling, and match arms `Pattern: statement`. `var` methods and `indirect` variants keep their markers. Formatting does not turn a proposed feature into implemented syntax.
