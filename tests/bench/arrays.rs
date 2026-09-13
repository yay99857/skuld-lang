// The same work as `arrays.skuld`, in Rust. See `README.md` for how to build.
//
// Deliberately written the way the Skuld version is — same generator, same
// comparison, same checksum — rather than the most idiomatic Rust, so that the
// two measure the same work and not two different programs.

fn next(state: i64) -> i64 {
    (state * 1664525 + 1013904223) % 2147483648
}

fn main() {
    let count: i64 = std::env::args()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let mut values: Vec<i64> = Vec::new();
    let mut state: i64 = 12345;
    for _ in 0..count {
        state = next(state);
        values.push(state % 1000000);
    }
    values.sort_by(|a, b| a.cmp(b));
    let mut checksum: i64 = 0;
    for (index, value) in values.iter().enumerate() {
        checksum = (checksum + value * (index as i64 % 7 + 1)) % 1000000007;
    }
    println!("{checksum}");
}
