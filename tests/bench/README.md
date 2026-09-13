# Benchmarks

Programs that measure something. They are **not** part of the test suite: a
measurement is not an expectation, and nothing here runs automatically.

```bash
tests/bench/run.sh          # build everything and print a table
```

The script builds the compiler in release mode if it has to, then builds each
benchmark in Skuld and in whichever of Rust and Go is installed, prints every
version's checksum, and times the fastest of three runs. Sizes can be raised
without editing anything:

```bash
ARRAYS_N=2000000 JSON_ROUNDS=1000 tests/bench/run.sh
```

The results from one machine are in [`BENCHMARKS.md`](../../BENCHMARKS.md), with
the toolchain that produced them.

## The rule these follow

Each benchmark exists in Skuld and, where a fair comparison is possible, in
Rust and Go. **The three must print the same checksum**, which is why every one
of them prints a number derived from the whole result before it is timed. If
the checksums disagree the programs are not doing the same work and the timings
mean nothing.

That is also why the Rust and Go versions are written the way the Skuld one is
— same generator, same loop, same comparison — rather than in the most
idiomatic form of their language. The subject is the language's cost for a
given piece of work, not who writes the prettiest program.

| Benchmark | What it measures | Rust | Go |
| --- | --- | --- | --- |
| `arrays` | Array growth and a stable sort through a comparator | yes | yes |
| `strings` | Building a string byte by byte, then scanning it | yes | yes |
| `dispatch` | Dynamic dispatch through an interface | yes | yes |
| `map_lookup` | A string-keyed index, against the array search it replaced | yes | yes |
| `json_parse` | Parsing `data/records.json` into a tree | no | yes |

`json_parse` has no Rust version on purpose: Rust's standard library has no
JSON, so the comparison would be against a third-party crate rather than
against a language. Go's is `encoding/json` decoding into `map[string]any`,
which allocates a generic tree and validates its input like Skuld's parser
does, but is not the same code — that difference is real and is reported
rather than hidden.

## Inputs

`data/records.json` is checked in: 61 KB, 400 records with nested objects,
arrays, floats, booleans and nulls. Everything else is generated inside the
programs by the same linear congruential generator in all three languages, so
no benchmark depends on a file that might differ.
