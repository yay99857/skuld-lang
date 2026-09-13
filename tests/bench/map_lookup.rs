// The same work as `map_lookup.skuld`, in Rust. See `README.md`.
//
// `linear` is the parallel-array search the map replaced; `map` is the
// language's own hash map, which is what Skuld's `std/map` is measured
// against.
use std::collections::HashMap;

fn keys_of(count: usize) -> Vec<String> {
    (0..count).map(|index| format!("key-{index}-name")).collect()
}

fn linear_total(keys: &[String]) -> usize {
    let mut names: Vec<&String> = Vec::new();
    let mut values: Vec<usize> = Vec::new();
    for (index, key) in keys.iter().enumerate() {
        names.push(key);
        values.push(index);
    }
    let mut total = 0;
    for key in keys {
        for position in 0..names.len() {
            if names[position] == key {
                total += values[position];
                break;
            }
        }
    }
    total
}

fn map_total(keys: &[String]) -> usize {
    let mut index_of: HashMap<&str, usize> = HashMap::new();
    for (index, key) in keys.iter().enumerate() {
        index_of.insert(key.as_str(), index);
    }
    let mut total = 0;
    for key in keys {
        if let Some(value) = index_of.get(key.as_str()) {
            total += value;
        }
    }
    total
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let count: usize = std::env::args()
        .nth(2)
        .and_then(|text| text.parse().ok())
        .unwrap_or(0);
    let keys = keys_of(count);
    if mode == "map" {
        println!("{}", map_total(&keys));
    } else {
        println!("{}", linear_total(&keys));
    }
}
