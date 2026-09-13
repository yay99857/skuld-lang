// The same work as `strings.skuld`, in Rust. See `README.md`.

fn push_number(buffer: &mut Vec<u8>, value: i64) {
    let mut digits: Vec<u8> = Vec::new();
    let mut rest = value;
    if rest == 0 {
        digits.push(48);
    }
    while rest > 0 {
        digits.push(48 + (rest % 10) as u8);
        rest /= 10;
    }
    for index in (0..digits.len()).rev() {
        buffer.push(digits[index]);
    }
}

fn main() {
    let count: i64 = std::env::args()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let mut buffer: Vec<u8> = Vec::new();
    for index in 0..count {
        buffer.extend_from_slice(b"item-");
        push_number(&mut buffer, index);
        buffer.push(59);
    }
    // Validated, like Skuld's `bytes_to_string`: the check is part of the work.
    let text = String::from_utf8(buffer).expect("utf-8");
    let bytes = text.as_bytes();
    let mut found = 0;
    let mut at = 0;
    while at + 1 < bytes.len() {
        if bytes[at] == 57 && bytes[at + 1] == 59 {
            found += 1;
        }
        at += 1;
    }
    println!("{} {}", text.len(), found);
}
