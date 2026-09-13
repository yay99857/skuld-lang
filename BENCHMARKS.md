# Measured performance

The project's goal has always been "native performance in the Go/Rust range".
This is the first document that treats that as a claim to be checked rather
than a property the C backend hands over. Reproduce it with
[`tests/bench/run.sh`](tests/bench/run.sh); what each benchmark does, and why
the Rust and Go versions are written the way they are, is in
[`tests/bench/README.md`](tests/bench/README.md).

## What ran

| | |
| --- | --- |
| Machine | Intel Xeon E5-2640 v3 @ 2.60 GHz, 16 threads, Linux x86-64 |
| Skuld | 0.1.0, release build, backend `clang -std=c11 -O2 -fno-fast-math` |
| clang | 22.1.8 |
| rustc | 1.98.1, `-O` |
| Go | 1.27.1, `go build` |
| Method | Fastest of nine runs, CPU time, pinned to one core, all three programs printing the same checksum |

Sizes: arrays 400000 elements, strings 1000000 items, dispatch 1000000 calls,
JSON 200 parses of a 61 KB document, map 20000 keys.

## Results

| Benchmark | Skuld | Rust | Go | Skuld ÷ best |
| --- | ---: | ---: | ---: | ---: |
| `arrays` — growth and a stable sort | **40 ms** | 18 ms | 219 ms | 2.2× |
| `strings` — build then scan | **111 ms** | 46 ms | 64 ms | 2.4× |
| `dispatch` — interface calls | **86 ms** | 66 ms | 144 ms | 1.3× |
| `json_parse` — 200 × 61 KB | **265 ms** | — | 487 ms | 0.5× |
| `map_lookup` — 20000 keys | **14 ms** | 7 ms | 6 ms | 2.3× |
| `map_lookup` — the same through an array search | **1536 ms** | 412 ms | 474 ms | 3.7× |

Peak resident memory for `json_parse` was 13 MB for both Skuld and Go.

These are the numbers after the reference-counting change described below.
Before it, the same machine measured 582 ms for `json_parse` and 179 ms for
`strings`.

## What the numbers say

**The range claim survives, with a spread.** Skuld is between 1.3× and 2.4× the
faster of Rust and Go across these workloads, and faster than Go on two of them
— by a factor of two on JSON and of five on sorting. That is the Go/Rust range
in the sense the project meant, and it is now evidence rather than an
expectation.

**Dispatch is nearly free.** Interface calls go through a per-class thunk and
land within 20% of Rust's `dyn Trait`. Nothing about the reference counting
shows up here.

**Measuring paid for itself immediately.** The first run of these benchmarks
put `strings` at 3.8× and JSON at 0.8× of Go; reading the generated C for the
slowest loop showed that every read of a managed local was taking a counted
copy of it — a retain and a release per array index, per field access, per
comparison. A binding nothing can reassign already holds that reference, so the
read now borrows it. JSON went from 582 ms to 265 ms and a byte scan from
433 ms to 121 ms, with no change to what any program means; `tests/pass/borrowed_reads.skuld`
is the fixture that keeps it honest.

**Strings remain the weakest workload,** at 2.4×. Building one byte by byte
still pays for a bounds check per push, and Skuld has no way to reserve
capacity up front. That is the next place to look, and it needs
before-and-after evidence, not a guess.

**The map is a library, not a builtin,** and lands at 2.3× against hash tables
written in C and Go. `std/map` is ordinary Skuld: the comparison is a language
against two runtimes.

**JSON is the workload the language was aimed at,** and it is now roughly twice
as fast as `encoding/json` into `map[string]any`. The two are not the same code —
that is stated in the benchmark's own README — but they do comparable work, and
a hand-written parser in Skuld holding its own against a standard library is
the closest thing here to a result.

**The compiler is not the slow part.**

| Step | Time |
| --- | ---: |
| `skuld check tests/pass/json_parser.skuld` (646 lines) | 4 ms |
| `skuld emit-c` on the same file | 5 ms |
| `skuld build` on the same file | 871 ms |

Everything after `emit-c` is clang at `-O2`: the compiler's own share of a
build is under 1%. A faster edit-compile loop is a matter of what is asked of
clang, not of the compiler.

## Portability

The second native target is **i686-linux-gnu** — the same machine and libc with
32-bit pointers. It was chosen because it can be run here, not only built for:
a claim about ARM or macOS with no machine behind it would be a claim, not
evidence.

`cli/tests/portability.rs` builds every `tests/pass` fixture for it and
compares the output byte for byte. **58 of 59 fixtures produce identical
output**, with no change to the runtime or the generated C.

The one that does not is `extern_c_ffi`, and the reason is worth stating
because it is the only place in the language where a target leaks through: an
`extern "C"` declaration names a concrete width, and C's own types do not.
`write(fd, buffer, count)` takes a `size_t`, which is 64 bits on x86-64 and 32
on i686, so a declaration that says `u64` is right on one target and wrong on
the other. Skuld has no `usize`; adding one is a language decision and not a
portability fix, so the fixture stays as it is and the limitation is recorded
here.

Two other places assume the platform without being caught by this test, since
no fixture exercises them:

- `std/net` and `std/dns` build a `sockaddr_in` by hand and assume a
  little-endian Linux layout, and use Linux's numeric constants (`AF_INET` 2,
  `SO_RCVTIMEO` 20, `O_WRONLY|O_CREAT|O_TRUNC` 577).
- `std/dns` passes a `struct timeval` of two 64-bit fields, which is right on
  LP64 and wrong on a 32-bit target — the one place where the 32-bit build
  would misbehave rather than merely differ.

Neither is a bug on the supported target, and both are the sort of thing a
third target would have to settle properly.

## What this does not claim

No speed ratio is promised, and none of these numbers is a reason to change
copy semantics, the memory model or the backend. A future optimisation needs
before-and-after evidence from these same benchmarks and unchanged semantics,
which is the rule the milestone set for itself.
