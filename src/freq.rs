use std::collections::HashMap;

// Count how many times ech byte value appears in the data

pub fn build_freq_table(data: &[u8]) -> HashMap<u8, u32> {
    let mut freqs = HashMap::new();
    for &byte in data {
        *freqs.entry(byte).or_insert(0) += 1;
    }
    freqs //return
}
