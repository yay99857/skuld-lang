---
title: Process & I/O
description: Print values, read process arguments and execute programs without a shell.
status: Implemented
---
The builtin `print` writes a value and a newline. `print()` writes a blank line. File I/O lives in `std/fs`, and process operations live in `std/os`.

## Arguments

`os.arguments() -> Result<[]string, OsError>` includes the program name. `os.parameters() -> Result<[]string, OsError>` provides the remaining arguments. Pass arguments with `skuld run main.skuld --args ...`.

## Subprocesses

```skuld
func run(program: string, arguments: []string) -> Result<Output, RunError>
func environment(name: string) -> Option<string>
```

These signatures belong to `std/os`. `run` looks up the executable on PATH, waits for it and captures standard output up to 64 KiB. `Output` contains the exit status, captured text and whether output was truncated. Standard error is inherited.

There is no shell expansion, quoting or pipeline syntax. A missing program produces status 127; a signalled process produces -1. Windows refuses `.bat` and `.cmd` files. Programs with their own nonstandard Windows command-line parser may interpret arguments differently.

## Flush and exit

`os.flush()` flushes output. `os.exit(code)` ends the process after flushing; it does not release managed values because the process is ending. The normal `main` signature still returns void.

## OS error details

`OsFailure` records the operation and OS error number. Use `describe_failure` for text. Library-specific error enums carry these details when an OS operation fails.
