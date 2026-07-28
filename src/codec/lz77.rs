use std::io::{self, Read, Write};
use anyhow::Result;

use crate::codec::Codec;
use crate::bitio::BitReader;

pub const WINDOW_SIZE: usize = 32768;
pub const MAX_MATCH: usize = 258;
pub const MIN_MATCH: usize = 3;
const HASH_BITS: usize = 15;
const HASH_SIZE: usize = 1 << HASH_BITS;
const HASH_MASK: usize = HASH_SIZE - 1;
const MAX_CHAIN: u32 = 32;

pub const LENGTH_TABLE: [(u16, u8); 29] = [
    (3, 0), (4, 0), (5, 0), (6, 0), (7, 0), (8, 0), (9, 0), (10, 0),
    (11, 1), (13, 1), (15, 1), (17, 1),
    (19, 2), (23, 2), (27, 2), (31, 2),
    (35, 3), (43, 3), (51, 3), (59, 3),
    (67, 4), (83, 4), (99, 4), (115, 4),
    (131, 5), (163, 5), (195, 5), (227, 5),
    (258, 0),
];

pub const DISTANCE_TABLE: [(u16, u8); 30] = [
    (1, 0), (2, 0), (3, 0), (4, 0),
    (5, 1), (7, 1),
    (9, 2), (13, 2),
    (17, 3), (25, 3),
    (33, 4), (49, 4),
    (65, 5), (97, 5),
    (129, 6), (193, 6),
    (257, 7), (385, 7),
    (513, 8), (769, 8),
    (1025, 9), (1537, 9),
    (2049, 10), (3073, 10),
    (4097, 11), (6145, 11),
    (8193, 12), (12289, 12),
    (16385, 13), (24577, 13),
];

pub fn encode_length(len: u16) -> (usize, u32) {
    // Length 258 has a dedicated code (index 28, 0 extra bits)
    if len == 258 {
        return (28, 0);
    }
    for (i, &(base, extra)) in LENGTH_TABLE.iter().enumerate() {
        if extra == 0 {
            if len == base {
                return (i, 0);
            }
        } else if len < base + (1 << extra) {
            return (i, (len - base) as u32);
        }
    }
    (28, 0)
}

pub fn encode_distance(dist: u16) -> (usize, u32) {
    for (i, &(base, extra)) in DISTANCE_TABLE.iter().enumerate() {
        if dist < base + (1 << extra) || i == 29 {
            return (i, (dist - base) as u32);
        }
    }
    (29, 0)
}

pub fn decode_length(idx: usize, extra: u32) -> u16 {
    let (base, _) = LENGTH_TABLE[idx];
    base + extra as u16
}

pub fn decode_distance(idx: usize, extra: u32) -> u16 {
    let (base, _) = DISTANCE_TABLE[idx];
    base + extra as u16
}

fn hash_three(data: &[u8]) -> usize {
    (((data[0] as usize) << 10) ^ ((data[1] as usize) << 5) ^ (data[2] as usize)) & HASH_MASK
}

struct HashChain {
    head: [i32; HASH_SIZE],
    prev: [i32; WINDOW_SIZE],
}

impl HashChain {
    fn new() -> Self {
        HashChain {
            head: [-1; HASH_SIZE],
            prev: [-1; WINDOW_SIZE],
        }
    }

    fn reset(&mut self) {
        for h in &mut self.head {
            *h = -1;
        }
        for p in &mut self.prev {
            *p = -1;
        }
    }

    fn add(&mut self, hash: usize, pos: u32) {
        let idx = pos as usize % WINDOW_SIZE;
        self.prev[idx] = self.head[hash];
        self.head[hash] = pos as i32;
    }
}

#[derive(Debug, Clone, Copy)]
pub enum LzToken {
    Literal(u8),
    Match { length: u16, distance: u16 },
}

pub struct Lz77Encoder {
    chain: HashChain,
    pub history: Vec<u8>,
    pub pos: u32,
    pub total_in: u64,
    pub total_literals: u64,
    pub total_matches: u64,
    pub total_match_len: u64,
}

impl Lz77Encoder {
    pub fn new() -> Self {
        Lz77Encoder {
            chain: HashChain::new(),
            history: Vec::with_capacity(WINDOW_SIZE),
            pos: 0,
            total_in: 0,
            total_literals: 0,
            total_matches: 0,
            total_match_len: 0,
        }
    }

    pub fn reset(&mut self) {
        self.chain.reset();
        self.history.clear();
        self.pos = 0;
        self.total_literals = 0;
        self.total_matches = 0;
        self.total_match_len = 0;
    }

    pub fn total_in(&self) -> u64 {
        self.total_in
    }

    fn history_byte(&self, abs_pos: u32) -> u8 {
        let oldest = self.pos - self.history.len() as u32;
        self.history[(abs_pos - oldest) as usize]
    }

    fn match_len(&self, chunk: &[u8], candidate: u32) -> usize {
        let mut len = 0usize;
        let pos = self.pos;
        let oldest = pos - self.history.len() as u32;

        while len < chunk.len() && len < MAX_MATCH {
            let cand_byte = {
                let cand_abs = candidate + len as u32;
                if cand_abs < pos {
                    self.history[(cand_abs - oldest) as usize]
                } else {
                    chunk[(cand_abs - pos) as usize]
                }
            };
            if cand_byte != chunk[len] {
                break;
            }
            len += 1;
        }
        len
    }

    pub fn find_match(&self, chunk: &[u8]) -> Option<(u16, u16)> {
        if chunk.len() < MIN_MATCH {
            return None;
        }

        let hash = hash_three(chunk);
        let head = self.chain.head[hash];
        if head < 0 {
            return None;
        }

        let pos = self.pos;
        let mut best_len = MIN_MATCH - 1;
        let mut best_dist = 0u16;
        let mut chain_count = 0u32;

        let mut candidate = head as u32;
        loop {
            let dist = pos - candidate;
            if dist > WINDOW_SIZE as u32 {
                break;
            }

            let ml = self.match_len(chunk, candidate);
            if ml > best_len {
                best_len = ml;
                best_dist = dist as u16;
                if best_len >= MAX_MATCH {
                    break;
                }
            }

            chain_count += 1;
            if chain_count >= MAX_CHAIN {
                break;
            }

            let p = self.chain.prev[candidate as usize % WINDOW_SIZE];
            if p < 0 {
                break;
            }
            candidate = p as u32;
        }

        if best_len >= MIN_MATCH {
            Some((best_len as u16, best_dist))
        } else {
            None
        }
    }

    pub fn push_byte(&mut self, byte: u8) {
        self.history.push(byte);
        if self.history.len() > WINDOW_SIZE {
            self.history.remove(0);
        }
        self.pos += 1;
    }

    pub fn add_hash_for(&mut self, chunk: &[u8], chunk_pos: usize, stream_pos: u32) {
        if chunk_pos + 2 < chunk.len() {
            let h = hash_three(&chunk[chunk_pos..]);
            self.chain.add(h, stream_pos);
        }
    }

    pub fn process<F: FnMut(LzToken)>(&mut self, chunk: &[u8], mut on_token: F) {
        let mut i = 0usize;
        while i < chunk.len() {
            let avail = &chunk[i..];
            if let Some((len, dist)) = self.find_match(avail) {
                on_token(LzToken::Match { length: len, distance: dist });
                self.total_matches += 1;
                self.total_match_len += len as u64;
                for j in 0..len as usize {
                    self.push_byte(avail[j]);
                    self.add_hash_for(chunk, i + j, self.pos - 1);
                    self.total_in += 1;
                }
                i += len as usize;
            } else {
                on_token(LzToken::Literal(avail[0]));
                self.total_literals += 1;
                self.push_byte(avail[0]);
                self.add_hash_for(chunk, i, self.pos - 1);
                self.total_in += 1;
                i += 1;
            }
        }
    }

    pub fn encode(&mut self, chunk: &[u8], bit_out: &mut Lz77BitWriter) {
        self.process(chunk, |token| match token {
            LzToken::Literal(byte) => {
                bit_out.write_bit(false);
                bit_out.write_bits_value(byte as u32, 8);
            }
            LzToken::Match { length, distance } => {
                bit_out.write_bit(true);
                let (len_idx, extra_len) = encode_length(length);
                bit_out.write_bits_value(len_idx as u32, 5);
                let (_, extra_bits) = LENGTH_TABLE[len_idx];
                bit_out.write_bits_value(extra_len, extra_bits);
                let (dist_idx, extra_dist) = encode_distance(distance);
                bit_out.write_bits_value(dist_idx as u32, 5);
                let (_, extra_dist_bits) = DISTANCE_TABLE[dist_idx];
                bit_out.write_bits_value(extra_dist, extra_dist_bits);
            }
        });
    }
}

pub struct Lz77BitWriter {
    pub buf: Vec<u8>,
    pub current_byte: u8,
    pub bits_filled: u8,
}

impl Lz77BitWriter {
    pub fn new() -> Self {
        Lz77BitWriter {
            buf: Vec::new(),
            current_byte: 0,
            bits_filled: 0,
        }
    }

    pub fn write_bit(&mut self, bit: bool) {
        if bit {
            self.current_byte |= 1 << (7 - self.bits_filled);
        }
        self.bits_filled += 1;
        if self.bits_filled == 8 {
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bits_filled = 0;
        }
    }

    pub fn write_bits_value(&mut self, mut value: u32, nbits: u8) {
        for _ in 0..nbits {
            self.write_bit((value & 1) != 0);
            value >>= 1;
        }
    }

    pub fn flush(&mut self) {
        if self.bits_filled > 0 {
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bits_filled = 0;
        }
    }
}

pub struct Lz77Codec {
    encoder: Lz77Encoder,
    bit_out: Lz77BitWriter,
}

impl Lz77Codec {
    pub fn new() -> Self {
        Lz77Codec {
            encoder: Lz77Encoder::new(),
            bit_out: Lz77BitWriter::new(),
        }
    }
}

impl Codec for Lz77Codec {
    fn method_id(&self) -> u8 {
        1
    }

    fn name(&self) -> &str {
        "lz77"
    }

    fn feed(&mut self, _chunk: &[u8]) -> Result<()> {
        Ok(())
    }

    fn finalize_feed(&mut self) -> Result<()> {
        Ok(())
    }

    fn write_header(&self, _output: &mut dyn Write) -> Result<()> {
        Ok(())
    }

    fn read_header(&mut self, _input: &mut dyn Read) -> Result<()> {
        Ok(())
    }

    fn report(&self) {
        let total = self.encoder.total_literals + self.encoder.total_matches;
        if total > 0 {
            let avg = if self.encoder.total_matches > 0 {
                self.encoder.total_match_len as f64 / self.encoder.total_matches as f64
            } else {
                0.0
            };
            println!("lz77: {} literals, {} matches (avg match len {:.1})", self.encoder.total_literals, self.encoder.total_matches, avg);
        } else {
            println!("lz77: no data encoded yet");
        }
    }

    fn encode_chunk(&mut self, chunk: &[u8], output: &mut dyn Write) -> Result<()> {
        self.encoder.encode(chunk, &mut self.bit_out);

        if !self.bit_out.buf.is_empty() {
            output.write_all(&self.bit_out.buf)?;
            self.bit_out.buf.clear();
        }
        Ok(())
    }

    fn finalize_encode(&mut self, output: &mut dyn Write) -> Result<()> {
        self.bit_out.flush();
        if !self.bit_out.buf.is_empty() {
            output.write_all(&self.bit_out.buf)?;
            self.bit_out.buf.clear();
        }
        output.flush()?;
        Ok(())
    }

    fn decoder<'a>(&'a self, input: Box<dyn Read + 'a>, original_len: u64) -> Box<dyn Read + 'a> {
        Box::new(Lz77Decoder {
            bits: BitReader::new(input),
            history: Vec::with_capacity(WINDOW_SIZE),
            remaining: original_len,
            pending_total: 0,
            pending_copied: 0,
            pending_distance: 0,
        })
    }
}

struct Lz77Decoder<R: Read> {
    bits: BitReader<R>,
    history: Vec<u8>,
    remaining: u64,
    pending_total: u16,
    pending_copied: u16,
    pending_distance: u16,
}

impl<R: Read> Lz77Decoder<R> {
    fn copy_pending(&mut self, buf: &mut [u8], written: &mut usize, distance: usize) {
        let remaining = (self.pending_total - self.pending_copied) as usize;
        let space = buf.len() - *written;
        let to_copy = remaining.min(space);
        let start = self.history.len().saturating_sub(distance);
        for i in 0..to_copy {
            buf[*written] = self.history[start + i];
            *written += 1;
            self.remaining -= 1;
            self.pending_copied += 1;
            self.history.push(buf[*written - 1]);
        }
        trim_history(&mut self.history);
        if self.pending_copied >= self.pending_total {
            self.pending_total = 0;
            self.pending_copied = 0;
        }
    }

    fn decode_match(&mut self) -> io::Result<(u16, u16)> {
        let len_idx = read_bits_value(&mut self.bits, 5)? as usize;
        let (base_len, extra_bits) = LENGTH_TABLE[len_idx];
        let extra_len = if extra_bits > 0 { read_bits_value(&mut self.bits, extra_bits)? } else { 0 };
        let total_len = base_len + extra_len as u16;

        let dist_idx = read_bits_value(&mut self.bits, 5)? as usize;
        let (base_dist, extra_dist_bits) = DISTANCE_TABLE[dist_idx];
        let extra_dist = if extra_dist_bits > 0 { read_bits_value(&mut self.bits, extra_dist_bits)? } else { 0 };
        let distance = base_dist + extra_dist as u16;

        Ok((total_len, distance))
    }
}

impl<R: Read> Read for Lz77Decoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut written = 0usize;

        if self.pending_total > 0 {
            self.copy_pending(buf, &mut written, self.pending_distance as usize);
        }

        while written < buf.len() && self.remaining > 0 {
            let is_match = self.bits.read_bit()?.ok_or_else(|| {
                io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected end of LZ77 bitstream")
            })?;

            if !is_match {
                let byte = read_bits_value(&mut self.bits, 8)? as u8;
                buf[written] = byte;
                written += 1;
                self.remaining -= 1;
                push_history(&mut self.history, byte);
            } else {
                let (total_len, distance) = self.decode_match()?;
                let capped = (total_len as u64).min(self.remaining) as u16;
                self.pending_total = capped;
                self.pending_copied = 0;
                self.pending_distance = distance;

                self.copy_pending(buf, &mut written, distance as usize);
                if self.pending_total > 0 {
                    break;
                }
            }
        }
        Ok(written)
    }
}

fn read_bits_value<R: Read>(bits: &mut BitReader<R>, n: u8) -> io::Result<u32> {
    let mut value = 0u32;
    for i in 0..n {
        let bit = bits.read_bit()?.ok_or_else(|| {
            io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected end of bitstream")
        })?;
        if bit {
            value |= 1 << i;
        }
    }
    Ok(value)
}

fn push_history(history: &mut Vec<u8>, byte: u8) {
    history.push(byte);
    if history.len() > WINDOW_SIZE {
        history.remove(0);
    }
}

fn trim_history(history: &mut Vec<u8>) {
    if history.len() > WINDOW_SIZE {
        let excess = history.len() - WINDOW_SIZE;
        history.drain(0..excess);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn roundtrip(data: &[u8]) {
        let mut codec = Lz77Codec::new();
        codec.finalize_feed().unwrap();

        let mut compressed = Vec::new();
        codec.encode_chunk(data, &mut compressed).unwrap();
        codec.finalize_encode(&mut compressed).unwrap();

        let mut decoder = codec.decoder(Box::new(compressed.as_slice()), data.len() as u64);
        let mut output = Vec::new();
        decoder.read_to_end(&mut output).unwrap();

        assert_eq!(output, data);
    }

    #[test]
    fn round_trips_small_input() {
        roundtrip(b"hello");
    }

    #[test]
    fn round_trips_repeated_pattern() {
        roundtrip(b"abcabcabcabcabc");
    }

    #[test]
    fn round_trips_single_byte() {
        roundtrip(b"x");
    }

    #[test]
    fn round_trips_identical_bytes() {
        let data = vec![b'a'; 1000];
        roundtrip(&data);
    }

    #[test]
    fn round_trips_large_window_crossing() {
        let mut data = Vec::with_capacity(70000);
        for i in 0..70000 {
            data.push((i % 251) as u8);
        }
        roundtrip(&data);
    }

    #[test]
    fn round_trips_all_literals() {
        let mut data = Vec::with_capacity(1000);
        for i in 0..1000 {
            data.push((i * 157) as u8);
        }
        roundtrip(&data);
    }

    #[test]
    fn matches_repeated_bytes() {
        let mut codec = Lz77Codec::new();
        let data = b"abcabcabc";
        let mut compressed = Vec::new();
        codec.encode_chunk(data, &mut compressed).unwrap();
        codec.finalize_encode(&mut compressed).unwrap();

        assert!(compressed.len() < data.len(), "LZ77 should compress repeated patterns");
    }

    #[test]
    fn encode_length_returns_correct_code() {
        assert_eq!(encode_length(3), (0, 0));
        assert_eq!(encode_length(4), (1, 0));
        assert_eq!(encode_length(11), (8, 0));
        assert_eq!(encode_length(12), (8, 1));
        assert_eq!(encode_length(258), (28, 0));
    }

    #[test]
    fn encode_distance_returns_correct_code() {
        assert_eq!(encode_distance(1), (0, 0));
        assert_eq!(encode_distance(5), (4, 0));
        assert_eq!(encode_distance(6), (4, 1));
        assert_eq!(encode_distance(32768), (29, 8191));
    }

    #[test]
    fn decode_length_matches_encode() {
        for len in 3..=258 {
            let (idx, extra) = encode_length(len);
            assert_eq!(decode_length(idx, extra), len);
        }
    }

    #[test]
    fn decode_distance_matches_encode() {
        for dist in 1..=32768u16 {
            if dist > 32768 { break; }
            let (idx, extra) = encode_distance(dist);
            assert_eq!(decode_distance(idx, extra), dist);
        }
    }
}
