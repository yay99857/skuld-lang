---
title: C interoperability
description: Declare foreign functions and make the unsafe boundary explicit.
status: Implemented
---
Foreign functions are declared in an `unsafe extern "C"` block. The compiler checks calls against that declaration, but cannot verify that the linked library has the signature you asserted.

## Foreign declarations

```skuld
unsafe extern "C" {
    func abs(value: i32) -> i32
}

func main() {
    print(int(abs(-3)))
}
```

The block requires `unsafe`; ordinary calls do not. Managed strings, arrays, classes, Options and Results cannot cross this boundary. Scalars, permitted raw pointers, explicit external layouts and void returns are supported.

## Borrowed pointers

`ptr(text)` yields `*u8`; `ptr(array)` borrows scalar element storage. No owner is retained. Pass a length separately and keep the underlying storage alive. Skuld strings are not NUL-terminated; use `std/cstring` where a C API needs a terminator.

## Unsafe pointer access

Inside lexical `unsafe { ... }` blocks, `load`, `store`, `volatile_load`, `volatile_store`, `offset`, `addr` and `ptr_from` provide explicit pointer access. Pointers address a scalar or void, not managed values. `ptr(local)` requires a scalar `var`.

Calling another function from an unsafe block does not make that function's body unsafe. The marker is lexical.

## Layout and the ABI

`extern struct` and `extern union` declare C-compatible layouts. `packed` and `align N` qualify an external struct's layout. `size_of(Type)` and `offset_of(Type, field)` produce `usize` values determined by the target C compiler; they are not compile-time constants in Skuld. Reading a union member requires `unsafe`.

## Linking and limits

Name libraries on the command line with `-l` and `-L`. C callbacks, varargs, pointers to pointers and non-C ABIs remain unsupported. Declarations that conflict with included C headers can fail at native compilation even if Skuld checking succeeds.
