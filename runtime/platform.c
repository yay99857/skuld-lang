/* The platform layer: everything a Skuld program needs from the operating
 * system that the foreign boundary cannot ask for directly.
 *
 * The boundary refuses a pointer to a pointer, a macro, and a function that
 * answers with a pointer into memory Skuld does not own — which rules out
 * `argv`, `errno` and `strerror` respectively. Each one is reached here
 * instead, and reshaped into what the boundary does accept: a count, a
 * length, a scalar, and a copy into bytes the caller already owns.
 *
 * That shape is also what makes a second operating system tractable. A
 * declaration written in Skuld is emitted verbatim as its own prototype, with
 * no header to check it against, so a width that is right on one system and
 * wrong on another links and mis-calls in silence. A definition here is
 * compiled against the real headers of the system being built for, which
 * turns that same mistake into a compile error. Platform differences belong
 * on this side of the line, behind names that do not change.
 *
 * These are not `static` and not prefixed with `skuld_`, because the standard
 * library declares them in an ordinary `unsafe extern "C"` block: a generated
 * name may not start with `skuld_`, and neither may a declared one. */

/* This file is compiled on its own, so it includes what it uses rather than
 * inheriting the program's prelude. That separation is the point: the headers
 * below declare `read`, `write`, `open` and `close`, and a program is allowed
 * to declare those itself through `extern "C"` — as `tests/pass/extern_c_ffi`
 * does. Inlining this file would make the two collide. */
#include <errno.h>
#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#ifdef _WIN32
#include <fcntl.h>
#include <io.h>
#include <sys/stat.h>
#else
#include <fcntl.h>
#include <unistd.h>
#endif

/* Arguments.
 *
 * A Skuld program cannot reach its own through `extern "C"` alone, since
 * `argv` is a pointer to a pointer. These three are the whole of it: a count,
 * a length, and a copy into bytes Skuld already owns. Nothing hands a pointer
 * back. */
static int skuld_argument_count = 0;
static char **skuld_argument_values = NULL;

/* Everything that has to happen before the program's own first statement.
 *
 * On Windows the C runtime opens the standard streams in text mode, which
 * rewrites every `\n` a program prints as `\r\n` on the way out. Skuld prints
 * bytes: `print` writes the string it was given and one newline, and a
 * program that writes a file expects back exactly what it wrote. A translated
 * stream would make the same source produce different bytes on two systems
 * for no reason the language admits to, so the streams are put into binary
 * mode and Skuld's output is LF everywhere. This is what Go does too.
 *
 * Nothing is needed on any other system: there is no translation to undo. */
void skuld_start(int argc, char **argv) {
    skuld_argument_count = argc;
    skuld_argument_values = argv;
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
}

int64_t sk_arg_count(void) { return (int64_t)skuld_argument_count; }

/* The length of one argument in bytes, or -1 where there is no such
 * argument, which is how a caller checks an index without trusting it. */
int64_t sk_arg_len(int64_t index) {
    if (index < 0 || index >= (int64_t)skuld_argument_count) return -1;
    return (int64_t)strlen(skuld_argument_values[index]);
}

/* Copy one argument into a buffer the caller owns, and report how many bytes
 * were written. A buffer that is too small is refused rather than truncated. */
int64_t sk_arg_copy(int64_t index, unsigned char *out, uint64_t capacity) {
    int64_t len = sk_arg_len(index);
    if (len < 0 || (uint64_t)len > capacity) return -1;
    memcpy(out, skuld_argument_values[index], (size_t)len);
    return len;
}

/* The two pointer operations that read no memory.
 *
 * A foreign function that allocates answers with NULL when it cannot, and one
 * that takes an optional callback wants NULL to say there is none. Skuld can
 * neither write a null pointer nor compare one, and giving it a way to would
 * mean giving it pointer arithmetic and dereferencing too. These two do
 * neither: one produces the null pointer, the other reports whether a pointer
 * is it. Nothing is read through a pointer here or anywhere else. */
void *sk_null(void) { return NULL; }

int64_t sk_is_null(void *value) { return value == NULL ? 1 : 0; }

/* The reason the last foreign call failed.
 *
 * `errno` is a macro over a function returning a pointer, and `strerror`
 * answers with one, so neither can cross the boundary as it is. Reading them
 * here keeps the rule intact: a number comes back, and a message is copied
 * into bytes the caller already owns. */
int64_t sk_errno(void) { return (int64_t)errno; }

int64_t sk_error_message(int64_t code, unsigned char *out, uint64_t capacity) {
    const char *text = strerror((int)code);
    size_t len = strlen(text);
    if (len > capacity) return -1;
    memcpy(out, text, len);
    return (int64_t)len;
}

/* Flush what has been printed so far. A test runner needs this: a program
 * that aborts loses whatever is still sitting in the buffer, and the line
 * that says which test was running is exactly what must survive. */
void sk_flush(void) { fflush(stdout); }

/* Exit with a status. The generated `main` flushes stdout before returning;
 * a program that leaves early has to flush here, or its output would be lost
 * in a pipe. Nothing is released: the process is ending. */
void sk_exit(int64_t code) {
    fflush(stdout);
    exit((int)code);
}

/* Files.
 *
 * `open`, `read` and `write` are POSIX spellings, and Windows numbers the
 * same ideas differently: `O_CREAT` is 64 on Linux and 256 there, `O_TRUNC`
 * agrees only by coincidence, and `O_BINARY` exists on one system and not the
 * other — omitting it on Windows translates every `\n` written to or read
 * from a file, which is precisely what a byte-exact API must not do. Written
 * as Skuld declarations those numbers have to be one system's, and the other
 * system has no header here to notice. Written here they are the header's.
 *
 * A handle crosses as an `i64` and a negative one means failure, which is
 * true of both systems. The caller closes what it opened; nothing here keeps
 * state between calls. */

int64_t sk_file_open(unsigned char *path, int64_t writing) {
#ifdef _WIN32
    int flags = writing ? (_O_WRONLY | _O_CREAT | _O_TRUNC | _O_BINARY) : (_O_RDONLY | _O_BINARY);
    return (int64_t)_open((const char *)path, flags, _S_IREAD | _S_IWRITE);
#else
    int flags = writing ? (O_WRONLY | O_CREAT | O_TRUNC) : O_RDONLY;
    return (int64_t)open((const char *)path, flags, 0644);
#endif
}

/* Both return the count, 0 at end of file, and negative for failure. The
 * Windows pair take an `unsigned int` rather than a `size_t`, so a request
 * larger than that is clamped rather than truncated to its low bits — the
 * caller loops anyway, and a short read is already part of the contract. */
int64_t sk_file_read(int64_t handle, unsigned char *buffer, uint64_t capacity) {
#ifdef _WIN32
    unsigned int want = capacity > (uint64_t)UINT_MAX ? UINT_MAX : (unsigned int)capacity;
    return (int64_t)_read((int)handle, buffer, want);
#else
    return (int64_t)read((int)handle, buffer, (size_t)capacity);
#endif
}

int64_t sk_file_write(int64_t handle, unsigned char *buffer, uint64_t count) {
#ifdef _WIN32
    unsigned int want = count > (uint64_t)UINT_MAX ? UINT_MAX : (unsigned int)count;
    return (int64_t)_write((int)handle, buffer, want);
#else
    return (int64_t)write((int)handle, buffer, (size_t)count);
#endif
}

void sk_file_close(int64_t handle) {
#ifdef _WIN32
    _close((int)handle);
#else
    close((int)handle);
#endif
}

/* One environment variable, copied into bytes the caller owns.
 *
 * `getenv` answers with a pointer into memory the program does not own, which
 * is why `std/os` read `/proc/self/environ` instead — a file exists on Linux
 * and the boundary can reach it. That workaround is no longer needed: the
 * pointer is read here and the bytes are copied out, so the answer is the
 * same on a system that has no `/proc`.
 *
 * -1 means unset, -2 means the buffer is too small; a caller can tell the two
 * apart, which it could not if both were simply "no". */
int64_t sk_environment(unsigned char *name, unsigned char *out, uint64_t capacity) {
    const char *value = getenv((const char *)name);
    if (value == NULL) return -1;
    size_t len = strlen(value);
    if ((uint64_t)len > capacity) return -2;
    memcpy(out, value, len);
    return (int64_t)len;
}
