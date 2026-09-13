#!/usr/bin/env bash
# Build and time every benchmark, in Skuld and in whichever of Rust and Go is
# installed. Run it from the repository root:
#
#     tests/bench/run.sh
#
# Each program is run three times and the fastest run is reported, which is the
# usual way to read a wall clock on a machine doing other things. Every program
# prints a checksum first: the Skuld, Rust and Go versions of a benchmark must
# print the same one, or they are not doing the same work and the numbers mean
# nothing.
set -u

root=$(cd "$(dirname "$0")/../.." && pwd)
bench="$root/tests/bench"
out="${TMPDIR:-/tmp}/skuld-bench-$$"
mkdir -p "$out"
trap 'rm -rf "$out"' EXIT

have() { command -v "$1" > /dev/null 2>&1; }

# The sizes each benchmark runs at. Bigger numbers make the differences
# clearer; these are chosen so the whole suite takes a couple of minutes.
ARRAYS_N=${ARRAYS_N:-400000}
STRINGS_N=${STRINGS_N:-1000000}
MAP_N=${MAP_N:-20000}
DISPATCH_N=${DISPATCH_N:-1000000}
JSON_ROUNDS=${JSON_ROUNDS:-200}

skuld="$root/target/release/skuld"
if [ ! -x "$skuld" ]; then
    echo "building the compiler in release mode"
    (cd "$root" && cargo build --release -p skuld-cli) || exit 1
fi

# The fastest of three runs, in milliseconds.
timed() {
    local best=""
    for _ in 1 2 3; do
        local start end elapsed
        start=$(date +%s%N)
        "$@" > /dev/null || return 1
        end=$(date +%s%N)
        elapsed=$(((end - start) / 1000000))
        if [ -z "$best" ] || [ "$elapsed" -lt "$best" ]; then best=$elapsed; fi
    done
    echo "$best"
}

report() { printf '%-14s %-6s %6s ms\n' "$1" "$2" "$3"; }

build_all() {
    local name=$1
    "$skuld" build "$bench/$name.skuld" -o "$out/${name}_skuld" || return 1
    if have rustc && [ -f "$bench/$name.rs" ]; then
        rustc -O -o "$out/${name}_rust" "$bench/$name.rs" 2> /dev/null
    fi
    if have go && [ -f "$bench/$name.go" ]; then
        (cd "$out" && cp "$bench/$name.go" . && go build -o "${name}_go" "$name.go" 2> /dev/null)
    fi
}

# Every benchmark prints its checksum before it is timed, so a mismatch is
# visible rather than silently measured.
checksums() {
    local name=$1
    shift
    for lang in skuld rust go; do
        [ -x "$out/${name}_$lang" ] || continue
        printf 'checksum %-10s %-6s %s\n' "$name" "$lang" "$("$out/${name}_$lang" "$@")"
    done
}

measure() {
    local name=$1
    shift
    for lang in skuld rust go; do
        [ -x "$out/${name}_$lang" ] || continue
        report "$name" "$lang" "$(timed "$out/${name}_$lang" "$@")"
    done
}

echo "== toolchain"
"$skuld" --version
clang --version | head -1
have rustc && rustc --version
have go && go version
echo

for name in arrays strings dispatch json_parse map_lookup; do
    build_all "$name"
done

echo "== checksums (the three languages must agree)"
checksums arrays "$ARRAYS_N"
checksums strings "$STRINGS_N"
checksums dispatch "$DISPATCH_N"
checksums json_parse "$bench/data/records.json" 1
checksums map_lookup map "$MAP_N"
echo

echo "== timings (fastest of three)"
measure arrays "$ARRAYS_N"
measure strings "$STRINGS_N"
measure dispatch "$DISPATCH_N"
measure json_parse "$bench/data/records.json" "$JSON_ROUNDS"
measure map_lookup map "$MAP_N"
# The array search the map replaced, for the same inputs.
for lang in skuld rust go; do
    [ -x "$out/map_lookup_$lang" ] || continue
    report "map(linear)" "$lang" "$(timed "$out/map_lookup_$lang" linear "$MAP_N")"
done
echo

echo "== the compiler itself"
report "check" "skuld" "$(timed "$skuld" check "$root/tests/pass/json_parser.skuld")"
report "emit-c" "skuld" "$(timed "$skuld" emit-c "$root/tests/pass/json_parser.skuld")"
report "build" "skuld" "$(timed "$skuld" build "$root/tests/pass/json_parser.skuld" -o "$out/json_parser")"
