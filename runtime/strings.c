/* Skuld managed strings. Embedded verbatim into generated C; this file is the
 * single source for retain/release, so the compiler never restates it.
 *
 * Reference counts are NOT atomic. Skuld has no threads, and paying for atomic
 * operations on every copy would be a cost with nothing to buy. Introducing
 * threads means revisiting this file, not the code generator.
 *
 * There is no cycle collector, by design: this is reference counting, not a
 * garbage collector. Strings are immutable and cannot form cycles, so nothing
 * here can leak; aggregates that can will need `weak` when they arrive. */

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
