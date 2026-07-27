use anyhow::{Context, Result, bail};
use std::collections::HashMap;
use std::io::Read;

use crate::meta::{FileMeta, read_meta_bytes, read_meta_from_reader, write_meta_bytes};

/// Bumped from BTREE1 -> BTREE2: the header now carries the original
/// file's metadata (mtime + permissions), so an old .bitree file fails
/// fast with a clear error instead of being misparsed.
const MAGIC: [u8; 6] = *b"BTREE2";

/// Everything read back out of a header, needed to decompress.
pub struct Header {
    pub freqs: HashMap<u8, u64>,
    pub original_len: u64,
    /// True if the compressed payload is a folder archive (built by
    /// archive.rs) rather than a single plain file's bytes.
    pub is_archive: bool,
    /// Metadata of the original input (the file, or the root folder
    /// when is_archive is true - per-file metadata for archive
    /// contents lives inside the archive payload itself, see archive.rs).
    pub meta: FileMeta,
}

/// Build the header bytes: magic + archive flag + frequency table +
/// original length. This does NOT include the compressed bitstream
/// itself - that gets appended separately by compress.rs.
pub fn write_header(
    freqs: &HashMap<u8, u64>,
    original_len: u64,
    is_archive: bool,
    meta: &FileMeta,
) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();

    // Magic number, so decompress can check "is this really our format?"
    for byte in MAGIC {
        out.push(byte);
    }

    // Single flag byte: 1 if this file was originally a folder (and
    // needs archive::extract_archive on the way out), 0 for a plain file.
    out.push(if is_archive { 1 } else { 0 });

    write_meta_bytes(&mut out, meta);

    // Number of distinct symbols, as 8 bytes (u64 little-endian).
    let symbol_count = freqs.len() as u64;
    for byte in symbol_count.to_le_bytes() {
        out.push(byte);
    }

    // Each symbol: 1 byte for the value, 8 bytes for its frequency.
    for (byte, freq) in freqs {
        out.push(*byte);

        for b in freq.to_le_bytes() {
            out.push(b);
        }
    }

    // Original file length, as 8 bytes (u64 little-endian).
    for byte in original_len.to_le_bytes() {
        out.push(byte);
    }

    out
}

/// Read a header back out of the start of a compressed file's bytes.
/// Returns the parsed Header, plus how many bytes the header took up
/// (so the caller knows where the compressed bitstream starts).
pub fn read_header(data: &[u8]) -> Result<(Header, usize)> {
    let mut pos: usize = 0;

    // Check the magic number matches what we expect.
    if data.len() < 6 {
        bail!("data too short to contain a magic number");
    }
    let magic = &data[0..6];
    if magic != MAGIC {
        bail!("not a valid bitree file (bad magic number)");
    }
    pos = pos + 6;

    // Read the archive flag (1 byte).
    if pos + 1 > data.len() {
        bail!("truncated header: missing archive flag byte");
    }
    let is_archive = data[pos] == 1;
    pos = pos + 1;

    // Handles the length checking and pos shifting in the function
    let meta = read_meta_bytes(data, &mut pos)?;

    // Read the symbol count (8 bytes, little-endian u64).
    if pos + 8 > data.len() {
        bail!("truncated header: missing symbol count");
    }
    let count_bytes = [
        data[pos],
        data[pos + 1],
        data[pos + 2],
        data[pos + 3],
        data[pos + 4],
        data[pos + 5],
        data[pos + 6],
        data[pos + 7],
    ];
    let symbol_count = u64::from_le_bytes(count_bytes);
    pos = pos + 8;

    // Read that many (byte, freq) pairs.
    let mut freqs: HashMap<u8, u64> = HashMap::new();
    let mut i: u64 = 0;
    while i < symbol_count {
        // Each entry is 1 byte (symbol) + 8 bytes (freq) = 9 bytes.
        if pos + 9 > data.len() {
            bail!("truncated header: symbol table cut short");
        }

        let byte_value = data[pos];
        pos = pos + 1;

        let freq_bytes = [
            data[pos],
            data[pos + 1],
            data[pos + 2],
            data[pos + 3],
            data[pos + 4],
            data[pos + 5],
            data[pos + 6],
            data[pos + 7],
        ];
        let freq_value = u64::from_le_bytes(freq_bytes);
        pos = pos + 8;

        freqs.insert(byte_value, freq_value);

        i = i + 1;
    }

    // Read the original file length (8 bytes, little-endian u64).
    if pos + 8 > data.len() {
        bail!("truncated header: missing original length");
    }
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
    pos = pos + 8;

    let header = Header {
        freqs: freqs,
        original_len: original_len,
        is_archive: is_archive,
        meta: meta,
    };

    Ok((header, pos))
}

/// Same as `read_header`, but reads from any `Read` stream instead of
/// requiring the whole (potentially huge) compressed file's bytes to
/// already be sitting in memory - decompress.rs uses this so it only
/// ever pulls in what the header actually needs.
pub fn read_header_from_reader<R: Read>(reader: &mut R) -> Result<Header> {
    let mut magic = [0u8; 6];
    reader
        .read_exact(&mut magic)
        .context("data too short to contain a magic number")?;
    if magic != MAGIC {
        bail!("not a valid bitree file (bad magic number)");
    }

    let mut flag_byte = [0u8; 1];
    reader
        .read_exact(&mut flag_byte)
        .context("truncated header: missing archive flag byte")?;
    let is_archive = flag_byte[0] == 1;

    let meta = read_meta_from_reader(reader).context("truncated header: missing metadata")?;

    let mut count_bytes = [0u8; 8];
    reader
        .read_exact(&mut count_bytes)
        .context("truncated header: missing symbol count")?;
    let symbol_count = u64::from_le_bytes(count_bytes);

    let mut freqs: HashMap<u8, u64> = HashMap::new();
    for _ in 0..symbol_count {
        // Each entry is 1 byte (symbol) + 8 bytes (freq) = 9 bytes.
        let mut entry_bytes = [0u8; 9];
        reader
            .read_exact(&mut entry_bytes)
            .context("truncated header: symbol table cut short")?;

        let byte_value = entry_bytes[0];
        let freq_bytes = [
            entry_bytes[1],
            entry_bytes[2],
            entry_bytes[3],
            entry_bytes[4],
            entry_bytes[5],
            entry_bytes[6],
            entry_bytes[7],
            entry_bytes[8],
        ];
        let freq_value = u64::from_le_bytes(freq_bytes);
        freqs.insert(byte_value, freq_value);
    }

    let mut len_bytes = [0u8; 8];
    reader
        .read_exact(&mut len_bytes)
        .context("truncated header: missing original length")?;
    let original_len = u64::from_le_bytes(len_bytes);

    Ok(Header {
        freqs,
        original_len,
        is_archive,
        meta,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_meta() -> FileMeta {
        FileMeta {
            modified_secs: 1_700_000_000,
            permissions: 0o644,
        }
    }

    #[test]
    fn round_trips_a_small_freq_table() {
        let mut freqs: HashMap<u8, u64> = HashMap::new();
        freqs.insert(b'a', 5);
        freqs.insert(b'b', 2);
        freqs.insert(b'c', 1);

        let header_bytes = write_header(&freqs, 8, false, &dummy_meta());
        let (parsed, header_size) = read_header(&header_bytes).unwrap();

        assert_eq!(parsed.original_len, 8);
        assert_eq!(parsed.is_archive, false);
        assert_eq!(parsed.freqs.get(&b'a'), Some(&5));
        assert_eq!(parsed.meta.modified_secs, 1_700_000_000);
        assert_eq!(parsed.meta.permissions, 0o644);
        assert_eq!(header_size, header_bytes.len());
    }

    #[test]
    fn round_trips_the_archive_flag() {
        let mut freqs: HashMap<u8, u64> = HashMap::new();
        freqs.insert(b'x', 1);

        let header_bytes = write_header(&freqs, 1, true, &dummy_meta());
        let (parsed, _) = read_header(&header_bytes).unwrap();

        assert_eq!(parsed.is_archive, true);
    }

    #[test]
    fn rejects_truncated_data() {
        let short = vec![1, 2, 3];
        assert!(read_header(&short).is_err());
    }
}
