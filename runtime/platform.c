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
 * and on Windows add `-lws2_32 -liphlpapi`, where the sockets and the
 * adapter list live outside the C library. `skuld build` and `skuld run`
 * pass those themselves. */

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

#ifdef _WIN32
/* <winsock2.h> must precede <windows.h>, which <io.h> may pull in. */
#include <winsock2.h>
#include <ws2tcpip.h>
/* After <winsock2.h>, which it depends on. */
#include <iphlpapi.h>

#include <fcntl.h>
#include <io.h>
#include <sys/stat.h>
#else
#include <arpa/inet.h>
#include <fcntl.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <sys/time.h>
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
