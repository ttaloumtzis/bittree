use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::io::{self, Read, Write};
use anyhow::Result;

use crate::codec::Codec;
use crate::codec::lz77::{self, Lz77Encoder, LENGTH_TABLE, DISTANCE_TABLE};
use crate::bitio::BitReader;

// ── Huffman tree for u16 symbols ──────────────────────────────────────

#[derive(Debug)]
enum HuffNode {
    Leaf { symbol: u16, freq: u64 },
    Internal { freq: u64, left: Box<HuffNode>, right: Box<HuffNode> },
}

impl HuffNode {
    fn freq(&self) -> u64 {
        match self {
            HuffNode::Leaf { freq, .. } => *freq,
            HuffNode::Internal { freq, .. } => *freq,
        }
    }
}

struct HeapNode {
    node: HuffNode,
    seq: u64,
}

impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        let freq_cmp = other.node.freq().cmp(&self.node.freq());
        if freq_cmp == Ordering::Equal {
            other.seq.cmp(&self.seq)
        } else {
            freq_cmp
        }
    }
}

impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for HeapNode {
    fn eq(&self, other: &Self) -> bool {
        self.node.freq() == other.node.freq() && self.seq == other.seq
    }
}

impl Eq for HeapNode {}

fn build_tree(freqs: &HashMap<u16, u64>) -> Option<HuffNode> {
    let mut entries: Vec<(u16, u64)> = freqs.iter().map(|(&s, &f)| (s, f)).collect();
    entries.sort_by_key(|e| e.0);

    let mut next_seq: u64 = 0;
    let mut heap: BinaryHeap<HeapNode> = BinaryHeap::new();

    for (symbol, freq) in entries {
        heap.push(HeapNode {
            node: HuffNode::Leaf { symbol, freq },
            seq: next_seq,
        });
        next_seq += 1;
    }

    if heap.len() == 1 {
        let only = heap.pop().unwrap().node;
        let freq = only.freq();
        let dummy = HuffNode::Leaf { symbol: 0, freq: 0 };
        return Some(HuffNode::Internal {
            freq,
            left: Box::new(only),
            right: Box::new(dummy),
        });
    }

    while heap.len() > 1 {
        let a = heap.pop().unwrap().node;
        let b = heap.pop().unwrap().node;
        let combined = a.freq() + b.freq();
        heap.push(HeapNode {
            node: HuffNode::Internal {
                freq: combined,
                left: Box::new(a),
                right: Box::new(b),
            },
            seq: next_seq,
        });
        next_seq += 1;
    }

    heap.pop().map(|h| h.node)
}

type CodeTable = HashMap<u16, Vec<bool>>;

fn build_codes(root: &HuffNode) -> CodeTable {
    let mut table = CodeTable::new();
    let mut path = Vec::new();
    walk(root, &mut path, &mut table);
    table
}

fn walk(node: &HuffNode, path: &mut Vec<bool>, table: &mut CodeTable) {
    match node {
        HuffNode::Leaf { symbol, .. } => {
            table.insert(*symbol, path.clone());
        }
        HuffNode::Internal { left, right, .. } => {
            path.push(false);
            walk(left, path, table);
            path.pop();
            path.push(true);
            walk(right, path, table);
            path.pop();
        }
    }
}

fn decode_symbol<R: Read>(node: &HuffNode, bits: &mut BitReader<R>) -> io::Result<u16> {
    let mut n = node;
    loop {
        match n {
            HuffNode::Leaf { symbol, .. } => return Ok(*symbol),
            HuffNode::Internal { left, right, .. } => {
                let bit = bits.read_bit()?.ok_or_else(|| {
                    io::Error::new(io::ErrorKind::UnexpectedEof, "unexpected end of Huffman bitstream")
                })?;
                n = if bit { right } else { left };
            }
        }
    }
}

struct HuffTree {
    root: HuffNode,
    codes: CodeTable,
}

impl HuffTree {
    fn from_freqs(freqs: &HashMap<u16, u64>) -> Self {
        let root = build_tree(freqs).expect("must have at least one symbol");
        let codes = build_codes(&root);
        HuffTree { root, codes }
    }

    fn encode(&self, symbol: u16) -> &[bool] {
        self.codes.get(&symbol).expect("symbol not found in Huffman tree")
    }

    fn decode<R: Read>(&self, bits: &mut BitReader<R>) -> io::Result<u16> {
        decode_symbol(&self.root, bits)
    }
}

// ── Bit-packing helpers (no output writer dependency) ─────────────────

struct BitPacker {
    buf: Vec<u8>,
    current_byte: u8,
    bits_filled: u8,
}

impl BitPacker {
    fn new() -> Self {
        BitPacker {
            buf: Vec::new(),
            current_byte: 0,
            bits_filled: 0,
        }
    }

    fn write_bit(&mut self, bit: bool) {
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

    fn write_bits_value(&mut self, mut value: u32, nbits: u8) {
        for _ in 0..nbits {
            self.write_bit((value & 1) != 0);
            value >>= 1;
        }
    }

    fn write_code(&mut self, code: &[bool]) {
        for &bit in code {
            self.write_bit(bit);
        }
    }

    fn flush(&mut self) {
        if self.bits_filled > 0 {
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bits_filled = 0;
        }
    }
}

// ── Deflate codec ─────────────────────────────────────────────────────

pub struct DeflateCodec {
    encoder: Lz77Encoder,
    ll_freqs: HashMap<u16, u64>,
    dist_freqs: HashMap<u16, u64>,
    ll_tree: Option<HuffTree>,
    dist_tree: Option<HuffTree>,
    packer: BitPacker,
}

impl DeflateCodec {
    pub fn new() -> Self {
        DeflateCodec {
            encoder: Lz77Encoder::new(),
            ll_freqs: HashMap::new(),
            dist_freqs: HashMap::new(),
            ll_tree: None,
            dist_tree: None,
            packer: BitPacker::new(),
        }
    }
}

impl Codec for DeflateCodec {
    fn method_id(&self) -> u8 {
        2
    }

    fn name(&self) -> &str {
        "deflate"
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<()> {
        let mut i = 0usize;
        while i < chunk.len() {
            let avail = &chunk[i..];
            if let Some((len, dist)) = self.encoder.find_match(avail) {
                let (len_idx, _) = lz77::encode_length(len);
                *self.ll_freqs.entry(257 + len_idx as u16).or_insert(0) += 1;
                let (dist_idx, _) = lz77::encode_distance(dist);
                *self.dist_freqs.entry(dist_idx as u16).or_insert(0) += 1;
                self.encoder.total_matches += 1;
                self.encoder.total_match_len += len as u64;

                for j in 0..len as usize {
                    self.encoder.push_byte(avail[j]);
                    self.encoder.add_hash_for(chunk, i + j, self.encoder.pos - 1);
                    self.encoder.total_in += 1;
                }
                i += len as usize;
            } else {
                *self.ll_freqs.entry(avail[0] as u16).or_insert(0) += 1;
                self.encoder.total_literals += 1;

                self.encoder.push_byte(avail[0]);
                self.encoder.add_hash_for(chunk, i, self.encoder.pos - 1);
                self.encoder.total_in += 1;
                i += 1;
            }
        }
        Ok(())
    }

    fn finalize_feed(&mut self) -> Result<()> {
        if self.ll_freqs.is_empty() && self.dist_freqs.is_empty() {
            return Ok(());
        }
        // Ensure at least one symbol exists in each tree
        if self.ll_freqs.is_empty() {
            self.ll_freqs.insert(0, 1);
        }
        if self.dist_freqs.is_empty() {
            self.dist_freqs.insert(0, 1);
        }
        self.ll_tree = Some(HuffTree::from_freqs(&self.ll_freqs));
        self.dist_tree = Some(HuffTree::from_freqs(&self.dist_freqs));
        self.encoder.reset();
        Ok(())
    }

    fn write_header(&self, output: &mut dyn Write) -> Result<()> {
        write_freq_map(output, &self.ll_freqs)?;
        write_freq_map(output, &self.dist_freqs)?;
        Ok(())
    }

    fn read_header(&mut self, input: &mut dyn Read) -> Result<()> {
        self.ll_freqs = read_freq_map(input)?;
        self.dist_freqs = read_freq_map(input)?;
        self.ll_tree = Some(HuffTree::from_freqs(&self.ll_freqs));
        self.dist_tree = Some(HuffTree::from_freqs(&self.dist_freqs));
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
            println!("deflate: {} literals, {} matches (avg match len {:.1})", self.encoder.total_literals, self.encoder.total_matches, avg);
        } else {
            println!("deflate: no data encoded yet");
        }
        if self.ll_tree.is_some() {
            println!("  LL Huffman tree: {} symbols", self.ll_freqs.len());
        }
        if self.dist_tree.is_some() {
            println!("  distance Huffman tree: {} symbols", self.dist_freqs.len());
        }
    }

    fn encode_chunk(&mut self, chunk: &[u8], output: &mut dyn Write) -> Result<()> {
        let ll_tree = self.ll_tree.as_ref().expect("must finalize_feed before encoding");
        let dist_tree = self.dist_tree.as_ref().expect("must finalize_feed before encoding");

        let mut i = 0usize;
        while i < chunk.len() {
            let avail = &chunk[i..];

            // Flush buffer when large
            if self.packer.buf.len() > 65536 {
                output.write_all(&self.packer.buf)?;
                self.packer.buf.clear();
            }

            if let Some((len, dist)) = self.encoder.find_match(avail) {
                let (len_idx, extra_len) = lz77::encode_length(len);
                let len_sym = 257 + len_idx as u16;
                self.packer.write_code(ll_tree.encode(len_sym));
                let (_, extra_bits) = LENGTH_TABLE[len_idx];
                self.packer.write_bits_value(extra_len, extra_bits);

                let (dist_idx, extra_dist) = lz77::encode_distance(dist);
                self.packer.write_code(dist_tree.encode(dist_idx as u16));
                let (_, extra_dist_bits) = DISTANCE_TABLE[dist_idx];
                self.packer.write_bits_value(extra_dist, extra_dist_bits);

                self.encoder.total_matches += 1;
                self.encoder.total_match_len += len as u64;
                for j in 0..len as usize {
                    self.encoder.push_byte(avail[j]);
                    self.encoder.add_hash_for(chunk, i + j, self.encoder.pos - 1);
                    self.encoder.total_in += 1;
                }
                i += len as usize;
            } else {
                self.packer.write_code(ll_tree.encode(avail[0] as u16));
                self.encoder.total_literals += 1;

                self.encoder.push_byte(avail[0]);
                self.encoder.add_hash_for(chunk, i, self.encoder.pos - 1);
                self.encoder.total_in += 1;
                i += 1;
            }
        }

        if !self.packer.buf.is_empty() {
            output.write_all(&self.packer.buf)?;
            self.packer.buf.clear();
        }
        Ok(())
    }

    fn finalize_encode(&mut self, output: &mut dyn Write) -> Result<()> {
        self.packer.flush();
        if !self.packer.buf.is_empty() {
            output.write_all(&self.packer.buf)?;
            self.packer.buf.clear();
        }
        output.flush()?;
        Ok(())
    }

    fn decoder<'a>(&'a self, input: Box<dyn Read + 'a>, original_len: u64) -> Box<dyn Read + 'a> {
        assert!(self.ll_tree.is_some(), "must call read_header or finalize_feed first");
        assert!(self.dist_tree.is_some(), "must call read_header or finalize_feed first");
        // Rebuild trees from freqs since HuffTree is not ref-counted and Codec returns &self
        let ll_tree = HuffTree::from_freqs(&self.ll_freqs);
        let dist_tree = HuffTree::from_freqs(&self.dist_freqs);
        Box::new(DeflateDecoder {
            bits: BitReader::new(input),
            ll_tree,
            dist_tree,
            history: Vec::with_capacity(lz77::WINDOW_SIZE),
            remaining: original_len,
            pending_total: 0,
            pending_copied: 0,
            pending_distance: 0,
        })
    }
}

// ── Deflate decoder ───────────────────────────────────────────────────

struct DeflateDecoder<R: Read> {
    bits: BitReader<R>,
    ll_tree: HuffTree,
    dist_tree: HuffTree,
    history: Vec<u8>,
    remaining: u64,
    pending_total: u16,
    pending_copied: u16,
    pending_distance: u16,
}

impl<R: Read> DeflateDecoder<R> {
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
}

impl<R: Read> Read for DeflateDecoder<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut written = 0usize;

        if self.pending_total > 0 {
            self.copy_pending(buf, &mut written, self.pending_distance as usize);
        }

        while written < buf.len() && self.remaining > 0 {
            let sym = self.ll_tree.decode(&mut self.bits)?;

            if sym < 256 {
                // Literal byte
                let byte = sym as u8;
                buf[written] = byte;
                written += 1;
                self.remaining -= 1;
                push_history(&mut self.history, byte);
            } else if sym >= 257 && sym <= 285 {
                let len_idx = (sym - 257) as usize;
                let (base_len, extra_bits) = LENGTH_TABLE[len_idx];
                let extra_val = if extra_bits > 0 {
                    read_raw_bits(&mut self.bits, extra_bits)?
                } else {
                    0
                };
                let total_len = base_len + extra_val as u16;

                let dist_sym = self.dist_tree.decode(&mut self.bits)?;
                let dist_idx = dist_sym as usize;
                let (base_dist, extra_dist_bits) = DISTANCE_TABLE[dist_idx];
                let extra_dist = if extra_dist_bits > 0 {
                    read_raw_bits(&mut self.bits, extra_dist_bits)?
                } else {
                    0
                };
                let distance = base_dist + extra_dist as u16;

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

// ── I/O helpers ───────────────────────────────────────────────────────

fn read_raw_bits<R: Read>(bits: &mut BitReader<R>, n: u8) -> io::Result<u32> {
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
    if history.len() > lz77::WINDOW_SIZE {
        history.remove(0);
    }
}

fn trim_history(history: &mut Vec<u8>) {
    if history.len() > lz77::WINDOW_SIZE {
        let excess = history.len() - lz77::WINDOW_SIZE;
        history.drain(0..excess);
    }
}

/// Write a frequency map: 2-byte LE count + N × [symbol(u16 LE) + freq(u64 LE)]
fn write_freq_map(output: &mut dyn Write, map: &HashMap<u16, u64>) -> Result<()> {
    let count = map.len() as u16;
    output.write_all(&count.to_le_bytes())?;
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|&(&k, _)| k);
    for &(&sym, &freq) in &entries {
        output.write_all(&sym.to_le_bytes())?;
        output.write_all(&freq.to_le_bytes())?;
    }
    Ok(())
}

/// Read a frequency map written by write_freq_map
fn read_freq_map(input: &mut dyn Read) -> Result<HashMap<u16, u64>> {
    let mut count_buf = [0u8; 2];
    input.read_exact(&mut count_buf)?;
    let count = u16::from_le_bytes(count_buf) as usize;

    let mut map = HashMap::with_capacity(count);
    let mut entry = [0u8; 10]; // 2-byte symbol + 8-byte freq
    for _ in 0..count {
        input.read_exact(&mut entry)?;
        let sym = u16::from_le_bytes(entry[0..2].try_into().unwrap());
        let freq = u64::from_le_bytes(entry[2..10].try_into().unwrap());
        map.insert(sym, freq);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn roundtrip(data: &[u8]) {
        let mut codec = DeflateCodec::new();

        codec.feed(data).unwrap();
        codec.finalize_feed().unwrap();

        let mut header = Vec::new();
        codec.write_header(&mut header).unwrap();

        let mut compressed = Vec::new();
        codec.encode_chunk(data, &mut compressed).unwrap();
        codec.finalize_encode(&mut compressed).unwrap();

        let mut dec = DeflateCodec::new();
        dec.read_header(&mut std::io::Cursor::new(&header)).unwrap();

        let input = compressed.as_slice();
        let mut decoder = dec.decoder(Box::new(input), data.len() as u64);
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
    fn round_trips_large_input() {
        let mut data = Vec::with_capacity(70000);
        for i in 0..70000 {
            data.push((i % 251) as u8);
        }
        roundtrip(&data);
    }

    #[test]
    fn deflate_outperforms_lz77_on_repeated_data() {
        let data = vec![b'a'; 10000];

        let mut lz77 = lz77::Lz77Codec::new();
        let mut lz77_out = Vec::new();
        lz77.encode_chunk(&data, &mut lz77_out).unwrap();
        lz77.finalize_encode(&mut lz77_out).unwrap();

        let mut def = DeflateCodec::new();
        def.feed(&data).unwrap();
        def.finalize_feed().unwrap();
        let mut header = Vec::new();
        def.write_header(&mut header).unwrap();
        let mut def_out = Vec::new();
        def.encode_chunk(&data, &mut def_out).unwrap();
        def.finalize_encode(&mut def_out).unwrap();
        let def_total = header.len() + def_out.len();

        // Deflate with Huffman should be smaller than standalone LZ77
        assert!(def_total < lz77_out.len(),
            "Deflate ({} bytes) should beat LZ77 ({} bytes) on repeated data",
            def_total, lz77_out.len());
    }

    #[test]
    fn feed_collects_frequencies() {
        let mut codec = DeflateCodec::new();
        codec.feed(b"abcdefghijklmnop").unwrap();
        codec.finalize_feed().unwrap();

        // All unique bytes — all are literals in LZ77
        for b in b'a'..=b'p' {
            assert_eq!(codec.ll_freqs.get(&(b as u16)), Some(&1),
                "byte {} should have freq 1", b);
        }
    }
}
