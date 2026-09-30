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

## Normalized syntax

Return types are written `-> Type`, the only accepted spelling, and match arms `Pattern: statement`. `var` methods and `indirect` variants keep their markers. Formatting does not turn a proposed feature into implemented syntax.
