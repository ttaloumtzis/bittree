use std::collections::HashMap;

const MAGIC: [u8; 6] = *b"BTREE1";

/// Everything read back out of a header, needed to decompress.
pub struct Header {
    pub freqs: HashMap<u8, u32>,
    pub original_len: u64,
}

/// Build the header bytes: magic + frequency table + original length.
/// This does NOT include the compressed bitstream itself - that gets
/// appended separately by compress.rs.
pub fn write_header(freqs: &HashMap<u8, u32>, original_len: u64) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();

    // Magic number, so decompress can check "is this really our format?"
    for byte in MAGIC {
        out.push(byte);
    }

    // Number of distinct symbols, as 4 bytes (u32 little-endian).
    let symbol_count = freqs.len() as u32;
    let count_bytes = symbol_count.to_le_bytes(); // [u8; 4]
    for byte in count_bytes {
        out.push(byte);
    }

    // Each symbol: 1 byte for the value, 4 bytes for its frequency.
    for (byte, freq) in freqs {
        out.push(*byte);

        let freq_bytes = freq.to_le_bytes(); // [u8; 4]
        for b in freq_bytes {
            out.push(b);
        }
    }

    // Original file length, as 8 bytes (u64 little-endian).
    let len_bytes = original_len.to_le_bytes(); // [u8; 8]
    for byte in len_bytes {
        out.push(byte);
    }

    out
}

/// Read a header back out of the start of a compressed file's bytes.
/// Returns the parsed Header, plus how many bytes the header took up
/// (so the caller knows where the compressed bitstream starts).
pub fn read_header(data: &[u8]) -> (Header, usize) {
    let mut pos: usize = 0;

    // Check the magic number matches what we expect.
    let found_magic = &data[0..6];
    assert_eq!(
        found_magic, MAGIC,
        "not a valid .bitree file (bad magic number)"
    );
    pos = pos + 6; //shift 6 pos since we checked Magic number

    // Read the symbol count (4 bytes, little-endian u32).
    let count_bytes = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
    let symbol_count = u32::from_le_bytes(count_bytes);
    pos = pos + 4; // shift 4 pos after reading the le count bytes

    // Read that many (byte, freq) pairs.
    let mut freqs: HashMap<u8, u32> = HashMap::new();

    let mut i: u32 = 0;
    while i < symbol_count {
        let byte_value = data[pos];
        pos = pos + 1;

        let freq_bytes = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
        let freq_value = u32::from_le_bytes(freq_bytes);
        pos = pos + 4; // shift 4 pos every freq u32 we read

        freqs.insert(byte_value, freq_value);

        i = i + 1;
    }

    // Read the original file length (8 bytes, little-endian u64).
    let len_bytes = [
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
        data[pos + 4],
        data[pos + 5],
        data[pos + 6],
        data[pos + 7],
    ];
    let original_len = u64::from_le_bytes(len_bytes);
    pos = pos + 8; // shift 8 pos after reading the u64 original len

    let header = Header {
        freqs: freqs,
        original_len: original_len,
    };

    (header, pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_a_small_freq_table() {
        let mut freqs: HashMap<u8, u32> = HashMap::new();
        freqs.insert(b'a', 5);
        freqs.insert(b'b', 2);
        freqs.insert(b'c', 1);

        let original_len: u64 = 8;

        let header_bytes = write_header(&freqs, original_len);
        let (parsed, header_size) = read_header(&header_bytes);

        assert_eq!(parsed.original_len, 8);
        assert_eq!(parsed.freqs.get(&b'a'), Some(&5));
        assert_eq!(parsed.freqs.get(&b'b'), Some(&2));
        assert_eq!(parsed.freqs.get(&b'c'), Some(&1));
        assert_eq!(header_size, header_bytes.len());
    }
}
