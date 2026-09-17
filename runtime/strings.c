/* Skuld managed memory. Embedded verbatim into generated C; this file is the
 * single source for retain/release, so the compiler never restates it.
 *
 * Reference counts are NOT atomic. Skuld has no threads, and paying for atomic
 * operations on every copy would be a cost with nothing to buy. Introducing
 * threads means revisiting this file, not the code generator.
 *
 * There is no cycle collector, by design: this is reference counting, not a
 * garbage collector. Strong cycles through classes/arrays require explicit
 * breaking or weak class back-links. */

typedef struct {
    size_t count;
    size_t len;
    unsigned char data[];
} skuld_buffer;

/* `owner` is NULL for string literals, whose bytes are static. Retain and
 * release are then no-ops, so literals still cost no allocation. */
typedef struct {
    const unsigned char *data;
    size_t len;
    skuld_buffer *owner;
} skuld_string;

static inline skuld_string skuld_string_retain(skuld_string value) {
    if (value.owner != NULL) {
        value.owner->count += 1;
    }
    return value;
}

/* Takes a pointer so it can be used as a `cleanup` handler, which is how every
 * owning slot is released on every exit path, including return and break. */
static inline void skuld_string_release(skuld_string *slot) {
    if (slot->owner != NULL && --slot->owner->count == 0) {
        free(slot->owner);
    }
    slot->owner = NULL;
}

/* Retains before releasing, so assigning a value to itself is safe. */
static inline void skuld_string_assign(skuld_string *slot, skuld_string value) {
    skuld_string previous = *slot;
    *slot = skuld_string_retain(value);
    skuld_string_release(&previous);
}

static inline skuld_string skuld_string_concat(skuld_string a, skuld_string b, size_t byte) {
    size_t len;
    if (__builtin_add_overflow(a.len, b.len, &len) || len + 1 < len) {
        skuld_fail("string length overflow", byte);
    }
    skuld_buffer *buffer = malloc(sizeof(skuld_buffer) + len);
    if (buffer == NULL) {
        skuld_fail("out of memory", byte);
    }
    buffer->count = 1;
    buffer->len = len;
    /* memcpy requires a valid pointer even for zero bytes. */
    if (a.len != 0) {
        memcpy(buffer->data, a.data, a.len);
    }
    if (b.len != 0) {
        memcpy(buffer->data + a.len, b.data, b.len);
    }
    return (skuld_string){buffer->data, len, buffer};
}

/* Conversions for interpolation. Each returns a string that owns itself,
 * except booleans, whose two spellings are static and never allocate. */

static skuld_string skuld_string_from_bytes(const char *bytes, size_t len) {
    skuld_buffer *buffer = malloc(sizeof(skuld_buffer) + len);
    if (buffer == NULL) {
        skuld_fail("out of memory", 0);
    }
    buffer->count = 1;
    buffer->len = len;
    if (len != 0) {
        memcpy(buffer->data, bytes, len);
    }
    return (skuld_string){buffer->data, len, buffer};
}

static inline skuld_string skuld_string_from_int(int64_t value) {
    char digits[32];
    int len = snprintf(digits, sizeof digits, "%" PRId64, value);
    if (len < 0) {
        skuld_fail("could not format an integer", 0);
    }
    return skuld_string_from_bytes(digits, (size_t)len);
}

/* Unsigned values are formatted through their own width so a u64 above
 * INT64_MAX still reads as itself rather than as a negative number. */
static inline skuld_string skuld_string_from_uint(uint64_t value) {
    char digits[32];
    int len = snprintf(digits, sizeof digits, "%" PRIu64, value);
    if (len < 0) {
        skuld_fail("could not format an integer", 0);
    }
    return skuld_string_from_bytes(digits, (size_t)len);
}

/* Matches `print`, so a value reads the same interpolated or printed. */
static inline skuld_string skuld_string_from_float(double value) {
    char digits[64];
    int len = snprintf(digits, sizeof digits, "%.17g", value);
    if (len < 0) {
        skuld_fail("could not format a float", 0);
    }
    return skuld_string_from_bytes(digits, (size_t)len);
}

static inline skuld_string skuld_string_from_bool(bool value) {
    return value ? (skuld_string){(const unsigned char *)"true", 4, NULL}
                 : (skuld_string){(const unsigned char *)"false", 5, NULL};
}

static inline skuld_string skuld_string_from_char(uint32_t cp) {
    char bytes[4];
    size_t len = 0;
    if (cp <= 0x7F) {
        bytes[0] = (char)cp;
        len = 1;
    } else if (cp <= 0x7FF) {
        bytes[0] = (char)(0xC0 | (cp >> 6));
        bytes[1] = (char)(0x80 | (cp & 0x3F));
        len = 2;
    } else if (cp <= 0xFFFF) {
        bytes[0] = (char)(0xE0 | (cp >> 12));
        bytes[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        bytes[2] = (char)(0x80 | (cp & 0x3F));
        len = 3;
    } else if (cp <= 0x10FFFF) {
        bytes[0] = (char)(0xF0 | (cp >> 18));
        bytes[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
        bytes[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
        bytes[3] = (char)(0x80 | (cp & 0x3F));
        len = 4;
    } else {
        skuld_fail("invalid Unicode code point", 0);
    }
    return skuld_string_from_bytes(bytes, len);
}


/* Shared allocation header for classes and arrays. One implicit weak count
 * keeps the header alive during destruction of the last strong reference. */
typedef struct skuld_object {
    size_t strong;
    size_t weak;
    void (*destroy)(struct skuld_object *);
} skuld_object;
typedef skuld_object *skuld_weak;

static inline void skuld_object_init(skuld_object *object, void (*destroy)(skuld_object *)) {
    object->strong = 1;
    object->weak = 1;
    object->destroy = destroy;
}
static inline void *skuld_object_retain(skuld_object *object) {
    if (object->strong == SIZE_MAX) skuld_fail("reference count overflow", 0);
    object->strong += 1;
    return object;
}
static inline skuld_weak skuld_weak_retain(skuld_weak value) {
    if (value != NULL) {
        if (value->weak == SIZE_MAX) skuld_fail("weak reference count overflow", 0);
        value->weak += 1;
    }
    return value;
}
static inline void skuld_weak_release(skuld_weak *slot) {
    if (*slot != NULL && --(*slot)->weak == 0) free(*slot);
    *slot = NULL;
}
static inline void skuld_weak_assign(skuld_weak *slot, skuld_weak value) {
    skuld_weak previous = *slot;
    *slot = skuld_weak_retain(value);
    skuld_weak_release(&previous);
}
static inline void skuld_object_release(skuld_object *object) {
    if (--object->strong == 0) {
        object->destroy(object);
        skuld_weak_release(&object);
    }
}
static inline bool skuld_weak_alive(skuld_weak value) {
    return value != NULL && value->strong != 0;
}
/* Check and retain together. Null stays internal and lowers to Option::None. */
static inline void *skuld_weak_upgrade(skuld_weak value) {
    if (!skuld_weak_alive(value)) return NULL;
    return skuld_object_retain(value);
}
static inline void *skuld_weak_get(skuld_weak value, size_t byte) {
    if (!skuld_weak_alive(value)) skuld_fail("expired weak reference", byte);
    return skuld_object_retain(value);
}
static inline void *skuld_allocate(size_t base, size_t count, size_t element, size_t byte) {
    size_t bytes;
    if (__builtin_mul_overflow(count, element, &bytes) ||
        __builtin_add_overflow(base, bytes, &bytes)) skuld_fail("allocation size overflow", byte);
    void *value = malloc(bytes);
    if (value == NULL) skuld_fail("out of memory", byte);
    return value;
}
/* One comparison covers both ends: a negative index becomes an enormous
 * unsigned value, which is already past any length. Clang was folding the two
 * into one anyway — measuring showed no difference — so this is for the reader
 * and for compilers that do not. */
static inline size_t skuld_index(int64_t index, size_t length, size_t byte) {
    if ((uint64_t)index >= length) skuld_fail("array index out of bounds", byte);
    return (size_t)index;
}

/* A half-open range, like every other range in the language: `start == end`
 * is the empty slice and `end == length` is the whole of it. */
static inline void skuld_slice_range(int64_t start, int64_t end, size_t length, size_t byte) {
    /* All three tests are load-bearing: `-3..2` passes the other two and would
     * slice five bytes out of a three-byte string. */
    if (start < 0 || end < start || (uint64_t)end > length)
        skuld_fail("slice out of bounds", byte);
}

static inline unsigned char skuld_string_byte(skuld_string value, int64_t index, size_t byte) {
    /* Negative wraps past the length, as in `skuld_index`. */
    if ((uint64_t)index >= value.len) skuld_fail("string index out of bounds", byte);
    return value.data[(size_t)index];
}

/* Slicing copies rather than retaining the owner: a three-byte view must not
 * keep a large buffer alive. Literal bytes are static and outlive every slice
 * of them, so those need no copy and no owner. */
static inline skuld_string skuld_string_slice(skuld_string value, int64_t start, int64_t end,
                                              size_t byte) {
    skuld_slice_range(start, end, value.len, byte);
    size_t len = (size_t)(end - start);
    if (value.owner == NULL) {
        return (skuld_string){value.data + start, len, NULL};
    }
    return skuld_string_from_bytes((const char *)(value.data + start), len);
}

/* Strict UTF-8: overlong encodings, surrogates and anything above U+10FFFF are
 * rejected, so a validated string really is what its consumers assume. */
static bool skuld_utf8_valid(const unsigned char *data, size_t len, size_t *offset) {
    size_t i = 0;
    while (i < len) {
        unsigned char lead = data[i];
        size_t extra;
        uint32_t code;
        if (lead < 0x80) {
            i += 1;
            continue;
        } else if ((lead & 0xE0) == 0xC0) {
            extra = 1;
            code = lead & 0x1Fu;
        } else if ((lead & 0xF0) == 0xE0) {
            extra = 2;
            code = lead & 0x0Fu;
        } else if ((lead & 0xF8) == 0xF0) {
            extra = 3;
            code = lead & 0x07u;
        } else {
            *offset = i;
            return false;
        }
        if (len - i <= extra) {
            *offset = i;
            return false;
        }
        for (size_t k = 1; k <= extra; ++k) {
            unsigned char next = data[i + k];
            if ((next & 0xC0) != 0x80) {
                *offset = i;
                return false;
            }
            code = (code << 6) | (next & 0x3Fu);
        }
        uint32_t lowest = extra == 1 ? 0x80u : (extra == 2 ? 0x800u : 0x10000u);
        if (code < lowest || code > 0x10FFFFu || (code >= 0xD800u && code <= 0xDFFFu)) {
            *offset = i;
            return false;
        }
        i += extra + 1;
    }
    return true;
}

/* Shared array identity stays fixed; only its separate element buffer moves.
 * Capacity grows geometrically. All lengths remain representable as Skuld int. */
static inline size_t skuld_array_next_length(size_t length, size_t byte) {
    if (length == SIZE_MAX || (uint64_t)length >= INT64_MAX)
        skuld_fail("array length overflow", byte);
    return length + 1;
}
static inline void *skuld_array_reserve(void *data, size_t *capacity,
                                       size_t needed, size_t element, size_t byte) {
    if (data != NULL && *capacity == 0) skuld_fail("cannot mutate a fixed array view", byte);
    if ((uint64_t)needed > INT64_MAX) skuld_fail("array length overflow", byte);
    if (needed <= *capacity) return data;
    size_t next = *capacity == 0 ? 4 : *capacity;
    while (next < needed) {
        if (next > SIZE_MAX / 2 || (uint64_t)next > INT64_MAX / 2) {
            next = needed;
            break;
        }
        next *= 2;
    }
    size_t bytes;
    if (__builtin_mul_overflow(next, element, &bytes))
        skuld_fail("allocation size overflow", byte);
    /* Empty value structs can have size zero in the current clang backend. */
    void *grown = realloc(data, bytes == 0 ? 1 : bytes);
    if (grown == NULL) skuld_fail("out of memory", byte);
    *capacity = next;
    return grown;
}
static inline size_t skuld_insert_index(int64_t index, size_t length, size_t byte) {
    /* One past the end is where an insertion is allowed to land; negative
     * still wraps past it. */
    if ((uint64_t)index > length) skuld_fail("array insertion index out of bounds", byte);
    return (size_t)index;
}

/* The process bridge: arguments and exit.
 *
 * Reading `argv` means following a pointer to a pointer, which the foreign
 * boundary deliberately refuses, so a Skuld program cannot reach its own
 * arguments through `extern "C"` alone. These four calls are the whole of the
 * bridge and they are deliberately shaped like the boundary already is: a
 * count, a length, and a copy into bytes Skuld already owns. Nothing here
 * hands a pointer back.
 *
 * They are not `static` and not prefixed with `skuld_`, because the standard
 * library declares them in an ordinary `unsafe extern "C"` block: a generated
 * name may not start with `skuld_`, and neither may a declared one. */
static int skuld_argument_count = 0;
static char **skuld_argument_values = NULL;

static void skuld_arguments_init(int argc, char **argv) {
    skuld_argument_count = argc;
    skuld_argument_values = argv;
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
