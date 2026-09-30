---
title: CLI reference
description: Find the commands and exit behavior of the skuld executable.
status: Implemented
---
Use `skuld --help` for the compiler's current command list. Options can appear on either side of the source file.

## Commands

| Command | Purpose |
| --- | --- |
| `skuld check file.skuld` | Full static checking without clang |
| `skuld run file.skuld` | Build, run and clean temporary output |
| `skuld build file.skuld` | Keep a native executable |
| `skuld emit-c file.skuld` | Emit checked C |
| `skuld lex file.skuld` | Inspect tokens |
| `skuld parse file.skuld` | Inspect syntax |
| `skuld resolve file.skuld` | Inspect name resolution |
| `skuld fmt file.skuld` | Format source in place |
| `skuld test file.skuld` | Execute top-level test functions |

## Options and program arguments

`-o` chooses the build output. `-l` and `-L` select native libraries and search directories. `--` ends compiler options. Only `run` accepts `--args`, after which arguments are passed unchanged to the program.

```bash
skuld run main.skuld --args input.json
skuld check -- -unusual-name.skuld
skuld --version
```

## Tests

A test is a top-level `func test_...()` with no parameters and no return value. A test file compiles once, and tests run in source order in one process. A failure stops the suite; there is no recoverable panic.

```skuld sample_tests.skuld
import "std/testing"

func test_addition() {
    testing.equal_int(2 + 2, 4, "addition")
}
```

```bash
skuld test sample_tests.skuld
```

## Exit codes

Successful compiler commands return 0, compiler errors return 1, and invalid CLI usage returns 2. `run` propagates the program's exit status; on Unix a signal maps to `128 + signal`. Help and version print to stdout.
