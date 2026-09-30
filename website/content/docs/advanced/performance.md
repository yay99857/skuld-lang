---
title: Performance
description: Measure native behavior without treating a backend choice as a performance guarantee.
status: Experimental
---
Skuld targets native performance, but its C backend is not itself evidence of speed. Compare equivalent programs and verify their output before drawing conclusions.

## Benchmark evidence

The repository's [BENCHMARKS.md](https://github.com/yay99857/skuld-lang/blob/main/BENCHMARKS.md) records workload-specific comparisons. The programs and runner live in `tests/bench`. Read that report with its environment and methodology; no universal speed ratio is promised here.

## Costs to understand

- Dynamic arrays grow geometrically and share storage through references.
- Concatenation allocates strings; literals reference static bytes.
- Structs and fixed arrays copy values.
- Function values use non-escaping environments.
- Owning references incur retain/release work.

## Profile real programs

Use a representative input and compare checksums or full output before timing. Keep compiler settings and native toolchains comparable. An optimization must preserve overflow checks, evaluation order and ownership semantics.

## Platform differences

Word-sized integers match the target pointer width. Foreign layouts and linked libraries are platform-specific. Read the [C boundary](/docs/advanced/interoperability/) before treating a measurement from one target as portable.
