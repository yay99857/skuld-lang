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
 * name may not start with `skuld_`, and neither may a declared one.
 *
 * Compiling this by hand, beside the program `skuld emit-c` produced:
 *
 *     clang -std=c11 -O2 program.c skuld_platform.c -o program
 *
 * and on Windows add `-lws2_32 -liphlpapi -lcrypt32`, where the sockets, the
 * adapter list and the certificate store live outside the C library. `skuld
 * build` and `skuld run` pass those themselves; this list is the one a caller
 * compiling by hand has to keep up with, so it is kept correct here. */

/* The CRT marks its own POSIX-named functions deprecated in favour of the
 * `_s` variants. `_open` is the one used here, and it is used deliberately:
 * the `_s` forms take different arguments on one system and do not exist on
 * the other, which is the divergence this file exists to absorb rather than
 * spread. The warning is turned off rather than answered. */
#ifdef _WIN32
#define _CRT_SECURE_NO_WARNINGS 1
#endif

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
/* `wcslen` and `wmemcpy`, for the wide command line Windows takes. */
#include <wchar.h>

#ifdef _WIN32
/* <winsock2.h> must precede <windows.h>, which <io.h> may pull in. */
#include <winsock2.h>
#include <ws2tcpip.h>
/* After <winsock2.h>, which it depends on. */
#include <iphlpapi.h>

#include <fcntl.h>
#include <io.h>
#include <sys/stat.h>
#include <windows.h>
#else
#include <arpa/inet.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/wait.h>
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
 * Nothing is needed on any other system: there is no translation to undo.
 *
 * Winsock is started here for a different reason: on Windows no socket call
 * works until it has been, and doing it once at the start means no call in
 * the socket section below has to wonder whether it has happened. */
static void skuld_sockets_start(void);

void skuld_start(int argc, char **argv) {
    skuld_argument_count = argc;
    skuld_argument_values = argv;
#ifdef _WIN32
    _setmode(_fileno(stdout), _O_BINARY);
    _setmode(_fileno(stderr), _O_BINARY);
#endif
    skuld_sockets_start();
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
#ifdef _WIN32
    /* `strerror` knows the C library's own numbers and nothing else, and a
     * socket failure is not one of them: Winsock reports in a range of its
     * own, where `WSAECONNREFUSED` is 10061 rather than 111. Asking the
     * system itself covers both, and answers in the user's language.
     *
     * The message arrives with a trailing newline, which belongs to a dialog
     * box rather than to a value being put inside a sentence. */
    char *text = NULL;
    DWORD length = FormatMessageA(FORMAT_MESSAGE_ALLOCATE_BUFFER | FORMAT_MESSAGE_FROM_SYSTEM
                                      | FORMAT_MESSAGE_IGNORE_INSERTS,
                                  NULL, (DWORD)code, 0, (char *)&text, 0, NULL);
    if (length == 0 || text == NULL) {
        if (text != NULL) LocalFree(text);
        return -1;
    }
    while (length > 0) {
        char last = text[length - 1];
        if (last != '\n' && last != '\r' && last != '.' && last != ' ') break;
        length--;
    }
    if ((uint64_t)length > capacity) {
        LocalFree(text);
        return -1;
    }
    memcpy(out, text, (size_t)length);
    LocalFree(text);
    return (int64_t)length;
#else
    const char *text = strerror((int)code);
    size_t len = strlen(text);
    if (len > capacity) return -1;
    memcpy(out, text, len);
    return (int64_t)len;
#endif
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

/* Sockets.
 *
 * Almost every disagreement between the two systems lives in this one area,
 * and none of it is visible from Skuld once it is behind these names.
 *
 * A Windows `SOCKET` is a `UINT_PTR`, not an `int`, and failure is
 * `INVALID_SOCKET` — all bits set — rather than -1, so the POSIX test
 * `handle < 0` is not merely wrong there, it is wrong in the direction that
 * accepts a failed call. Closing one is `closesocket`, since the descriptor
 * is not a file descriptor. Writing to a connection the peer has closed
 * raises SIGPIPE on POSIX and needs `MSG_NOSIGNAL` to say otherwise, while
 * Windows has no signal to suppress and rejects the flag. The receive
 * timeout is a `struct timeval` on one system and a `DWORD` of milliseconds
 * on the other, and the struct itself is eight bytes there and sixteen here.
 * And nothing works at all on Windows until `WSAStartup` has run.
 *
 * So a handle crosses as an `i64` with -1 for failure, a timeout crosses as
 * milliseconds, and an address crosses as its four octets. */

#ifdef _WIN32
typedef SOCKET skuld_socket;
#define SKULD_NO_SOCKET INVALID_SOCKET
#else
typedef int skuld_socket;
#define SKULD_NO_SOCKET (-1)
#endif

/* Winsock has to be started before any other call in this section, and the
 * program's first statement has not run yet when `skuld_start` does. Doing it
 * there rather than lazily means no socket call has to wonder. */
static void skuld_sockets_start(void) {
#ifdef _WIN32
    WSADATA data;
    WSAStartup(MAKEWORD(2, 2), &data);
#endif
}

/* The system's reason the last socket call failed.
 *
 * It is a separate question from `sk_errno` because Winsock answers it in a
 * separate place: it never touches `errno`, and its numbers are their own
 * range — `WSAECONNREFUSED` is 10061 where POSIX `ECONNREFUSED` is 111. A
 * caller that wants to explain a network failure has to ask here. */
int64_t sk_socket_error(void) {
#ifdef _WIN32
    return (int64_t)WSAGetLastError();
#else
    return (int64_t)errno;
#endif
}

/* Connect to an IPv4 address given as its four octets. `datagram` chooses UDP
 * over TCP, which is the whole of what `std/dns` needs that `std/net` does
 * not. Answers the handle, or -1. */
int64_t sk_socket_connect(unsigned char *octets, int64_t port, int64_t datagram) {
    skuld_socket handle = socket(AF_INET, datagram ? SOCK_DGRAM : SOCK_STREAM, 0);
    if (handle == SKULD_NO_SOCKET) return -1;
    struct sockaddr_in address;
    memset(&address, 0, sizeof address);
    address.sin_family = AF_INET;
    address.sin_port = htons((unsigned short)port);
    unsigned long packed = ((unsigned long)octets[0] << 24) | ((unsigned long)octets[1] << 16)
                           | ((unsigned long)octets[2] << 8) | (unsigned long)octets[3];
    address.sin_addr.s_addr = htonl(packed);
    if (connect(handle, (struct sockaddr *)&address, (int)sizeof address) != 0) {
        /* Closing the socket resets the last error on both systems, so the
         * reason the connect failed has to be saved across it — otherwise the
         * caller asks why and is told nothing happened. */
#ifdef _WIN32
        int reason = WSAGetLastError();
        closesocket(handle);
        WSASetLastError(reason);
#else
        int reason = errno;
        close(handle);
        errno = reason;
#endif
        return -1;
    }
    return (int64_t)handle;
}

/* One write. Answers what was written, or -1. The caller loops: a socket is
 * allowed to accept less than it was offered on both systems. */
int64_t sk_socket_send(int64_t handle, unsigned char *buffer, uint64_t length) {
    int want = length > (uint64_t)INT_MAX ? INT_MAX : (int)length;
#ifdef _WIN32
    int wrote = send((skuld_socket)handle, (const char *)buffer, want, 0);
#else
    /* Without MSG_NOSIGNAL a write to a closed connection kills the process
     * rather than answering, which is not an error a Skuld program could
     * catch. Windows has no such signal and rejects the flag. */
    ssize_t wrote = send((skuld_socket)handle, buffer, (size_t)want, MSG_NOSIGNAL);
#endif
    return (int64_t)wrote;
}

/* One read. 0 means the peer closed, which is how a body with no declared
 * length ends; -1 is a failure. */
int64_t sk_socket_recv(int64_t handle, unsigned char *buffer, uint64_t capacity) {
    int want = capacity > (uint64_t)INT_MAX ? INT_MAX : (int)capacity;
#ifdef _WIN32
    int got = recv((skuld_socket)handle, (char *)buffer, want, 0);
#else
    ssize_t got = recv((skuld_socket)handle, buffer, (size_t)want, 0);
#endif
    return (int64_t)got;
}

/* How long a read may block before it gives up, in milliseconds. A resolver
 * that waits for a server that will never answer is the reason this exists.
 * Answers 0, or -1 if the system refused. */
int64_t sk_socket_timeout(int64_t handle, int64_t milliseconds) {
#ifdef _WIN32
    DWORD value = (DWORD)milliseconds;
    int result = setsockopt((skuld_socket)handle, SOL_SOCKET, SO_RCVTIMEO, (const char *)&value,
                            (int)sizeof value);
#else
    struct timeval value;
    value.tv_sec = (long)(milliseconds / 1000);
    value.tv_usec = (long)((milliseconds % 1000) * 1000);
    int result = setsockopt((skuld_socket)handle, SOL_SOCKET, SO_RCVTIMEO, &value, sizeof value);
#endif
    return result == 0 ? 0 : -1;
}

void sk_socket_close(int64_t handle) {
#ifdef _WIN32
    closesocket((skuld_socket)handle);
#else
    close((skuld_socket)handle);
#endif
}

/* The machine's own DNS servers, as dotted quads separated by NUL bytes.
 *
 * The two systems do not merely store this differently, they store it in
 * different kinds of place: a text file on one, and a per-adapter structure
 * reached through an API on the other. A resolver written in Skuld can parse
 * either once it is text, so the answer crosses as text and the finding of it
 * stays here.
 *
 * Answers the number of bytes written, 0 when the machine lists none, and -1
 * when the buffer is too small. IPv6 servers are left out: this resolver
 * speaks IPv4, and an address it cannot use is not an answer. */
int64_t sk_dns_servers(unsigned char *out, uint64_t capacity) {
    uint64_t written = 0;
#ifdef _WIN32
    /* `GetAdaptersAddresses` wants a buffer it can grow into, and the usual
     * advice is 15 KB to avoid asking twice. */
    ULONG size = 15 * 1024;
    IP_ADAPTER_ADDRESSES *adapters = NULL;
    for (int attempt = 0; attempt < 3; attempt++) {
        adapters = (IP_ADAPTER_ADDRESSES *)malloc(size);
        if (adapters == NULL) return 0;
        ULONG result = GetAdaptersAddresses(AF_INET, GAA_FLAG_SKIP_ANYCAST | GAA_FLAG_SKIP_MULTICAST
                                                         | GAA_FLAG_SKIP_FRIENDLY_NAME,
                                            NULL, adapters, &size);
        if (result == ERROR_SUCCESS) break;
        free(adapters);
        adapters = NULL;
        if (result != ERROR_BUFFER_OVERFLOW) return 0;
    }
    if (adapters == NULL) return 0;
    for (IP_ADAPTER_ADDRESSES *adapter = adapters; adapter != NULL; adapter = adapter->Next) {
        if (adapter->OperStatus != IfOperStatusUp) continue;
        for (IP_ADAPTER_DNS_SERVER_ADDRESS *server = adapter->FirstDnsServerAddress; server != NULL;
             server = server->Next) {
            if (server->Address.lpSockaddr == NULL) continue;
            if (server->Address.lpSockaddr->sa_family != AF_INET) continue;
            struct sockaddr_in *address = (struct sockaddr_in *)server->Address.lpSockaddr;
            char text[16];
            unsigned char *octets = (unsigned char *)&address->sin_addr;
            int length = snprintf(text, sizeof text, "%u.%u.%u.%u", octets[0], octets[1], octets[2],
                                  octets[3]);
            if (length <= 0) continue;
            if (written + (uint64_t)length + 1 > capacity) {
                free(adapters);
                return -1;
            }
            memcpy(out + written, text, (size_t)length);
            written += (uint64_t)length;
            out[written++] = 0;
        }
    }
    free(adapters);
#else
    FILE *file = fopen("/etc/resolv.conf", "r");
    if (file == NULL) return 0;
    char line[512];
    while (fgets(line, (int)sizeof line, file) != NULL) {
        if (strncmp(line, "nameserver", 10) != 0) continue;
        const char *rest = line + 10;
        while (*rest == ' ' || *rest == '\t') rest++;
        size_t length = strcspn(rest, " \t\r\n");
        /* An IPv6 server is skipped rather than mis-parsed. */
        if (length == 0 || memchr(rest, ':', length) != NULL) continue;
        if (written + (uint64_t)length + 1 > capacity) {
            fclose(file);
            return -1;
        }
        memcpy(out + written, rest, length);
        written += (uint64_t)length;
        out[written++] = 0;
    }
    fclose(file);
#endif
    return (int64_t)written;
}

/* Running another program, and reading what it wrote.
 *
 * This is the one place where the two systems disagree about the shape of the
 * idea rather than about a name or a width. POSIX splits a launch into `fork`
 * and `execvp`: a copy of this process that then becomes the other program,
 * with the pipe wired up in between. Windows has no such copy — `CreateProcess`
 * starts the other program directly, and the handles it should inherit are
 * described up front. Neither can be written in terms of the other, which is
 * why `std/os` cannot hold both and this function exists.
 *
 * One consequence is worth stating rather than discovering. On Windows the
 * command line reaches the child as a single string and the *child* splits it,
 * so the caller's arguments have to be quoted on the way in. The quoting
 * implemented here is the algorithm `CommandLineToArgvW` documents, which is
 * what the C runtime startup uses, so it round-trips for any child that parses
 * its arguments the ordinary way. A child that reads `GetCommandLineW` and
 * splits it by hand can still see something else, and no caller-side quoting
 * can prevent that. On POSIX the arguments are handed over as an array and the
 * question does not arise.
 *
 * `arguments` is the program's arguments, NUL-separated, ending in an empty
 * one. Answers the number of bytes the child wrote — which may exceed
 * `capacity`, meaning the rest was read and discarded — or -1 if it could not
 * be started at all. `status` receives the exit status, 127 for a program that
 * was not found, the way a shell reports it, and -1 for one killed by a signal.
 */

#ifdef _WIN32

/* UTF-8 in, UTF-16 out, allocated. Windows takes wide strings and Skuld has
 * only UTF-8, so every path and argument crosses here. */
static wchar_t *skuld_widen(const char *text) {
    int length = MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, -1, NULL, 0);
    if (length <= 0) return NULL;
    wchar_t *wide = (wchar_t *)malloc((size_t)length * sizeof(wchar_t));
    if (wide == NULL) return NULL;
    if (MultiByteToWideChar(CP_UTF8, MB_ERR_INVALID_CHARS, text, -1, wide, length) <= 0) {
        free(wide);
        return NULL;
    }
    return wide;
}

/* Append one argument to a command line, quoted the way the runtime startup
 * will unquote it. An argument with no space, tab or quote in it needs none of
 * this and is appended as it stands. */
static int skuld_quote(wchar_t *out, size_t *at, size_t capacity, const wchar_t *argument) {
    size_t length = wcslen(argument);
    int plain = length > 0;
    for (size_t i = 0; i < length; i++) {
        if (argument[i] == L' ' || argument[i] == L'\t' || argument[i] == L'"') plain = 0;
    }
    if (plain) {
        if (*at + length + 1 >= capacity) return 0;
        wmemcpy(out + *at, argument, length);
        *at += length;
        return 1;
    }
    if (*at + 1 >= capacity) return 0;
    out[(*at)++] = L'"';
    for (size_t i = 0; i < length; i++) {
        size_t slashes = 0;
        while (i < length && argument[i] == L'\\') {
            slashes++;
            i++;
        }
        if (i == length) {
            /* Trailing backslashes precede the closing quote, so each has to
             * be doubled or the quote would be escaped by them. */
            if (*at + slashes * 2 + 1 >= capacity) return 0;
            for (size_t n = 0; n < slashes * 2; n++) out[(*at)++] = L'\\';
            break;
        }
        if (argument[i] == L'"') {
            if (*at + slashes * 2 + 2 >= capacity) return 0;
            for (size_t n = 0; n < slashes * 2; n++) out[(*at)++] = L'\\';
            out[(*at)++] = L'\\';
            out[(*at)++] = L'"';
        } else {
            if (*at + slashes + 1 + 1 >= capacity) return 0;
            for (size_t n = 0; n < slashes; n++) out[(*at)++] = L'\\';
            out[(*at)++] = argument[i];
        }
    }
    if (*at + 1 >= capacity) return 0;
    out[(*at)++] = L'"';
    return 1;
}

/* A batch file is refused rather than run. `cmd.exe` applies its own quoting
 * rules to what it is given, and they are not the ones quoted for above — an
 * argument containing `&` or `|` would be read as a command separator. There
 * is no safe caller-side escaping for that, so it is not offered. */
static int skuld_is_batch(const char *program) {
    size_t length = strlen(program);
    if (length < 4) return 0;
    const char *tail = program + length - 4;
    return _stricmp(tail, ".bat") == 0 || _stricmp(tail, ".cmd") == 0;
}

int64_t sk_process_run(unsigned char *program, unsigned char *arguments, unsigned char *out,
                       uint64_t capacity, int64_t *status) {
    *status = -1;
    if (skuld_is_batch((const char *)program)) return -1;

    wchar_t *wide_program = skuld_widen((const char *)program);
    if (wide_program == NULL) return -1;

    /* The command line begins with the program itself, quoted like any other
     * argument, because that is where the child expects to find `argv[0]`. */
    size_t line_capacity = 32768;
    wchar_t *line = (wchar_t *)malloc(line_capacity * sizeof(wchar_t));
    if (line == NULL) {
        free(wide_program);
        return -1;
    }
    size_t at = 0;
    int ok = skuld_quote(line, &at, line_capacity, wide_program);
    for (unsigned char *argument = arguments; ok && *argument != 0;) {
        size_t length = strlen((const char *)argument);
        wchar_t *wide = skuld_widen((const char *)argument);
        if (wide == NULL) {
            ok = 0;
            break;
        }
        if (at + 1 < line_capacity) {
            line[at++] = L' ';
            ok = skuld_quote(line, &at, line_capacity, wide);
        } else {
            ok = 0;
        }
        free(wide);
        argument += length + 1;
    }
    if (!ok) {
        free(line);
        free(wide_program);
        return -1;
    }
    line[at] = 0;

    /* Only the write end is inherited, and only standard output is redirected:
     * a program that complains still complains where a person can see it. */
    SECURITY_ATTRIBUTES inheritable;
    inheritable.nLength = sizeof inheritable;
    inheritable.lpSecurityDescriptor = NULL;
    inheritable.bInheritHandle = TRUE;
    HANDLE readable = NULL;
    HANDLE writable = NULL;
    if (!CreatePipe(&readable, &writable, &inheritable, 0)) {
        free(line);
        free(wide_program);
        return -1;
    }
    SetHandleInformation(readable, HANDLE_FLAG_INHERIT, 0);

    STARTUPINFOW startup;
    memset(&startup, 0, sizeof startup);
    startup.cb = sizeof startup;
    startup.dwFlags = STARTF_USESTDHANDLES;
    startup.hStdOutput = writable;
    startup.hStdError = GetStdHandle(STD_ERROR_HANDLE);
    startup.hStdInput = GetStdHandle(STD_INPUT_HANDLE);
    PROCESS_INFORMATION started;
    memset(&started, 0, sizeof started);

    /* The program is looked up the way the system looks one up, which includes
     * PATHEXT: `skuld run` finds `skuld.exe`. */
    BOOL launched = CreateProcessW(NULL, line, NULL, NULL, TRUE, 0, NULL, NULL, &startup, &started);
    free(line);
    free(wide_program);
    CloseHandle(writable);
    if (!launched) {
        CloseHandle(readable);
        /* A program that is not there is 127, as a shell would report it,
         * rather than an error the caller has to tell apart from a crash. */
        *status = 127;
        return 0;
    }

    uint64_t written = 0;
    uint64_t total = 0;
    for (;;) {
        char block[4096];
        DWORD got = 0;
        if (!ReadFile(readable, block, sizeof block, &got, NULL) || got == 0) break;
        total += got;
        if (written < capacity) {
            uint64_t room = capacity - written;
            uint64_t take = got < room ? got : room;
            memcpy(out + written, block, (size_t)take);
            written += take;
        }
    }
    CloseHandle(readable);
    WaitForSingleObject(started.hProcess, INFINITE);
    DWORD code = 0;
    GetExitCodeProcess(started.hProcess, &code);
    CloseHandle(started.hProcess);
    CloseHandle(started.hThread);
    *status = (int64_t)code;
    return (int64_t)total;
}

#else

int64_t sk_process_run(unsigned char *program, unsigned char *arguments, unsigned char *out,
                       uint64_t capacity, int64_t *status) {
    *status = -1;
    int handles[2];
    if (pipe(handles) != 0) return -1;

    /* Everything the child needs is built before the fork: after it, only
     * async-signal-safe work is allowed, and allocating is not. */
    size_t count = 1;
    for (unsigned char *argument = arguments; *argument != 0;) {
        count++;
        argument += strlen((const char *)argument) + 1;
    }
    char **argv = (char **)malloc((count + 1) * sizeof(char *));
    if (argv == NULL) {
        close(handles[0]);
        close(handles[1]);
        return -1;
    }
    argv[0] = (char *)program;
    size_t index = 1;
    for (unsigned char *argument = arguments; *argument != 0;) {
        argv[index++] = (char *)argument;
        argument += strlen((const char *)argument) + 1;
    }
    argv[index] = NULL;

    pid_t child = fork();
    if (child < 0) {
        free(argv);
        close(handles[0]);
        close(handles[1]);
        return -1;
    }
    if (child == 0) {
        dup2(handles[1], 1);
        close(handles[0]);
        close(handles[1]);
        execvp((const char *)program, argv);
        /* `_exit` rather than `exit`: a failed start must not flush the
         * buffers this process inherited a copy of. 127 is what a shell
         * reports for a program that is not there. */
        _exit(127);
    }
    free(argv);
    close(handles[1]);

    uint64_t written = 0;
    uint64_t total = 0;
    for (;;) {
        char block[4096];
        ssize_t got = read(handles[0], block, sizeof block);
        if (got <= 0) break;
        total += (uint64_t)got;
        if (written < capacity) {
            uint64_t room = capacity - written;
            uint64_t take = (uint64_t)got < room ? (uint64_t)got : room;
            memcpy(out + written, block, (size_t)take);
            written += take;
        }
    }
    close(handles[0]);
    int raw = 0;
    waitpid(child, &raw, 0);
    /* `WIFSIGNALED`/`WEXITSTATUS` as the bit layout rather than the macros,
     * so this reads the same as the status word it decodes. */
    *status = (raw & 127) != 0 ? -1 : (int64_t)((raw >> 8) & 255);
    return (int64_t)total;
}

#endif

/* Trust anchors.
 *
 * What a program should believe about a certificate is the one thing OpenSSL
 * cannot work out for itself on Windows. `SSL_CTX_set_default_verify_paths`
 * looks where a Unix installation keeps its bundle, finds nothing, clears the
 * error and reports success, so a program there reaches the handshake and
 * then refuses every certificate for want of an issuer. Windows does hold the
 * answer; it just keeps it in a place OpenSSL does not look.
 *
 * So this hands it over. PEM rather than DER, for two reasons. The boundary
 * carries bytes well and structures badly, and OpenSSL reads PEM from memory
 * without being told a length in a C `long` — which is four bytes here and
 * eight on Linux, a width Skuld has no way to spell once.
 *
 * The caller asks twice: once with no buffer to learn the length, then again
 * with one. A short buffer is not an error, it is the first call.
 *
 * What this cannot do is what Windows does when it verifies a chain itself:
 * fetch a root it does not have yet. An enumeration sees only what is already
 * on the machine, so a certificate whose root has never been needed here is
 * refused rather than fetched. That fails closed, which is the right
 * direction to be wrong in, and it is what Zig's standard library ships. */
int64_t sk_trust_anchors_pem(unsigned char *out, uint64_t capacity) {
#ifdef _WIN32
    HCERTSTORE store = CertOpenSystemStoreW(0, L"ROOT");
    if (store == NULL) return -1;
    uint64_t total = 0;
    PCCERT_CONTEXT certificate = NULL;
    while ((certificate = CertEnumCertificatesInStore(store, certificate)) != NULL) {
        DWORD needed = 0;
        if (!CryptBinaryToStringA(certificate->pbCertEncoded,
                                  certificate->cbCertEncoded,
                                  CRYPT_STRING_BASE64HEADER, NULL, &needed)) {
            continue;
        }
        /* `needed` counts the terminator this never copies out. */
        if (needed == 0) continue;
        if (out == NULL || total + (uint64_t)(needed - 1) > capacity) {
            total += (uint64_t)(needed - 1);
            continue;
        }
        char *text = (char *)malloc(needed);
        if (text == NULL) continue;
        DWORD written = needed;
        if (CryptBinaryToStringA(certificate->pbCertEncoded,
                                 certificate->cbCertEncoded,
                                 CRYPT_STRING_BASE64HEADER, text, &written)) {
            memcpy(out + total, text, written);
            total += (uint64_t)written;
        }
        free(text);
    }
    CertCloseStore(store, 0);
    return (int64_t)total;
#else
    /* Every other system this builds for keeps a bundle where OpenSSL already
     * looks, so there is nothing to add and saying so is the whole answer. */
    (void)out;
    (void)capacity;
    return 0;
#endif
}
