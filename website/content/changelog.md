<p class="eyebrow">LANGUAGE & TOOLCHAIN</p>

# Changelog

This page summarizes the current source state. The Cargo workspace reports version 0.1.0; these entries are not a claim that each milestone was separately released.

## Version 0.1.0

**Status: experimental, current source.** No release date is asserted by this documentation.

### Language

- Native compiler pipeline, static checking and actionable source diagnostics.
- Structs, classes, weak references, arrays, interfaces and enums.
- Option and Result, pattern matching and error propagation.
- Sized integers, word-sized integers, characters, bit operations and explicit conversions.
- Module imports, public exports, constants, statics and deferred cleanup.
- C foreign declarations, unsafe pointer access, external layouts and freestanding builds.

### Language refinements (M32)

- Return types are written only as `-> Type`. `: Type` is an error with a suggested fix.
- `None` is the only way to spell absence. `null` is an error with a suggested fix.
- `weak.get()` is removed; use `upgrade()` instead.
- Comparators return the builtin `Ordering`: use `compare`, `total_compare` for floats, and `then` for a second key.
- `result.map_err(f)` converts an error at the call.
- `.Variant` names an enum variant where the context expects that enum.
- Struct methods declared `var` can change `this`.
- An enum can contain itself through a variant marked `indirect`.

### Toolchain and library

- Build, run, check, format and test commands.
- Language server and local editor integrations.
- Strings, UTF-8, JSON, maps, files, process and networking modules.
- Linux and Windows hosted support, including HTTPS through OpenSSL on both.

## Development history

See the repository's [commit history](https://github.com/yay99857/skuld-lang/commits/main/) for individual changes and [roadmap](https://github.com/yay99857/skuld-lang/blob/main/ROADMAP.md) for milestone decisions. Planned work is not a release commitment.
