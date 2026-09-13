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
static inline size_t skuld_index(int64_t index, size_t length, size_t byte) {
    if (index < 0 || (uint64_t)index >= length) skuld_fail("array index out of bounds", byte);
    return (size_t)index;
}
