use std::collections::HashMap;

// Count how many times each byte value appears in the data

pub fn build_freq_table(data: &[u8]) -> HashMap<u8, u64> {
    let mut freqs = HashMap::new();
    for &byte in data {
        *freqs.entry(byte).or_insert(0) += 1;
    }
    freqs //return
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_repeated_bytes() {
        let data = b"aaabbc";
        let freqs = build_freq_table(data);
        assert_eq!(freqs.get(&b'a'), Some(&3));
        assert_eq!(freqs.get(&b'b'), Some(&2));
        assert_eq!(freqs.get(&b'c'), Some(&1));
    }
}
