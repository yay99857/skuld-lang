---
title: Library overview
description: Explore the small standard library embedded in the Skuld compiler.
status: Implemented
---
The standard library is written in Skuld and embedded in the compiler. Import a module under the reserved `std` prefix, then call it through its final path segment.

```skuld
import "std/strings"

func main() {
    print(strings.trim("  Hello, Skuld!  "))
}
```

## Modules

| Module | Purpose |
| --- | --- |
| `std/strings` | Byte-based search, trimming, splitting and joining |
| `std/utf8` | Strict UTF-8 validation, decoding and scalar counting |
| `std/cstring` | NUL-terminated byte buffers for C |
| `std/map` | String keys with integer values |
| `std/fs` | Whole-file operations and explicitly closed streaming files |
| `std/os` | Arguments, environment, subprocesses, exit and OS errors |
| `std/testing` | Assertions for test functions |
| `std/json` | JSON parsing, rendering and lookup |
| `std/net` | Blocking TCP connections |
| `std/dns` | IPv4 host-name resolution |
| `std/http` | Blocking HTTP/1.1 client |
| `std/tls`, `std/https` | Opt-in verified TLS and HTTPS using OpenSSL |
| `std/ffi` | Null pointer construction and inspection |

## API availability

This reference describes the current 0.1.0 source checkout. The compiler and library are experimental; “Implemented” is not a stability guarantee. Individual API introduction versions are not tracked separately here.

## Platform scope

Linux x86_64 and Windows x86_64 are supported, with module-specific restrictions. HTTPS works on both but needs OpenSSL installed. Networking is blocking, and general concurrency is not available.

## Dependencies

Ordinary library imports need no package installation. TLS and HTTPS require OpenSSL and explicit linking: `-lssl -lcrypto` on Linux, `-llibssl -llibcrypto` on Windows. A local `std` directory cannot replace the embedded sources.
