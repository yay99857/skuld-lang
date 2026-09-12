# Runtime

Reserved for future retain/release and string/array allocation support.
The current native core emits small printing, string-comparison and checked
integer-arithmetic helpers directly into generated C. Strings are views of
static literal bytes; there is no Skuld-managed heap allocation or ARC yet.
Do not introduce a separate runtime library until it serves an actual need.
