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

#ifdef _WIN32
/* `_setmode` is declared in <io.h>, and that header also declares `open`,
 * `read` and `write` — three names `std/fs` and `std/os` declare for
 * themselves, with widths that are right for POSIX and wrong here. Including
 * it turns those disagreements into `conflicting types` errors, which is the
 * right outcome and precisely what moving those calls into this file will
 * deliver. Until that lands, declaring the one function needed keeps this
 * change to the one thing it is about. `_fileno` comes from <stdio.h>, which
 * the prelude already includes. */
int _setmode(int fd, int mode);
#define SKULD_O_BINARY 0x8000
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
static void skuld_start(int argc, char **argv) {
    skuld_argument_count = argc;
    skuld_argument_values = argv;
#ifdef _WIN32
    _setmode(_fileno(stdout), SKULD_O_BINARY);
    _setmode(_fileno(stderr), SKULD_O_BINARY);
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
