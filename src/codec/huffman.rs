use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};
use std::io::{self, Read, Write};
use anyhow::Result;

use crate::codec::Codec;
use crate::bitio::BitReader;

#[derive(Debug)]
enum Node {
    Leaf { byte: u8, freq: u64 },
    Internal { freq: u64, left: Box<Node>, right: Box<Node> },
}

impl Node {
    fn freq(&self) -> u64 {
        match self {
            Node::Leaf { freq, .. } => *freq,
            Node::Internal { freq, .. } => *freq,
        }
    }
}

struct HeapNode {
    node: Node,
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

fn build_tree(freqs: &HashMap<u8, u64>) -> Option<Node> {
    let mut entries: Vec<(u8, u64)> = freqs.iter().map(|(&b, &f)| (b, f)).collect();
    entries.sort_by_key(|e| e.0);

    let mut next_seq: u64 = 0;
    let mut heap: BinaryHeap<HeapNode> = BinaryHeap::new();

    for (byte, freq) in entries {
        heap.push(HeapNode {
            node: Node::Leaf { byte, freq },
            seq: next_seq,
        });
        next_seq += 1;
    }

    if heap.len() == 1 {
        let only_node = heap.pop().unwrap().node;
        let freq = only_node.freq();
        let dummy = Node::Leaf { byte: 0, freq: 0 };
        return Some(Node::Internal {
            freq,
            left: Box::new(only_node),
            right: Box::new(dummy),
        });
    }

    while heap.len() > 1 {
        let a = heap.pop().unwrap().node;
        let b = heap.pop().unwrap().node;
        let combined = a.freq() + b.freq();
        heap.push(HeapNode {
            node: Node::Internal {
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

type CodeTable = HashMap<u8, Vec<bool>>;

fn build_codes(root: &Node) -> CodeTable {
    let mut table = CodeTable::new();
    let mut path = Vec::new();
    walk(root, &mut path, &mut table);
    table
}

fn walk(node: &Node, path: &mut Vec<bool>, table: &mut CodeTable) {
    match node {
        Node::Leaf { byte, .. } => {
            table.insert(*byte, path.clone());
        }
        Node::Internal { left, right, .. } => {
            path.push(false);
            walk(left, path, table);
            path.pop();
            path.push(true);
            walk(right, path, table);
            path.pop();
        }
    }
}

struct HuffmanDecoder<'a, R: Read> {
    tree: &'a Node,
    bits: BitReader<R>,
    remaining: u64,
}

impl<'a, R: Read> Read for HuffmanDecoder<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;
        while written < buf.len() && self.remaining > 0 {
            let mut node = self.tree;
            loop {
                match node {
                    Node::Leaf { byte, .. } => {
                        buf[written] = *byte;
                        written += 1;
                        self.remaining -= 1;
                        break;
                    }
                    Node::Internal { left, right, .. } => {
                        let bit = self.bits.read_bit()?.ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "ran out of bits before reaching original_len",
                            )
                        })?;
                        node = if bit { right } else { left };
                    }
                }
            }
        }
        Ok(written)
    }
}

pub struct HuffmanCodec {
    freqs: HashMap<u8, u64>,
    tree: Option<Node>,
    codes: Option<CodeTable>,
    current_byte: u8,
    bits_filled: u8,
    buf: Vec<u8>,
}

impl HuffmanCodec {
    pub fn new() -> Self {
        HuffmanCodec {
            freqs: HashMap::new(),
            tree: None,
            codes: None,
            current_byte: 0,
            bits_filled: 0,
            buf: Vec::new(),
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

    fn flush_bits(&mut self) {
        if self.bits_filled > 0 {
            self.buf.push(self.current_byte);
            self.current_byte = 0;
            self.bits_filled = 0;
        }
    }
}

impl Codec for HuffmanCodec {
    fn method_id(&self) -> u8 {
        0
    }

    fn name(&self) -> &str {
        "huffman"
    }

    fn feed(&mut self, chunk: &[u8]) -> Result<()> {
        for &byte in chunk {
            *self.freqs.entry(byte).or_insert(0) += 1;
        }
        Ok(())
    }

    fn finalize_feed(&mut self) -> Result<()> {
        if self.freqs.is_empty() {
            return Ok(());
        }
        self.tree = build_tree(&self.freqs);
        self.codes = self.tree.as_ref().map(|t| build_codes(t));
        Ok(())
    }

    fn write_header(&self, output: &mut dyn Write) -> Result<()> {
        let symbol_count = self.freqs.len() as u64;
        output.write_all(&symbol_count.to_le_bytes())?;
        for (byte, freq) in &self.freqs {
            output.write_all(&[*byte])?;
            output.write_all(&freq.to_le_bytes())?;
        }
        Ok(())
    }

    fn read_header(&mut self, input: &mut dyn Read) -> Result<()> {
        let mut count_bytes = [0u8; 8];
        input.read_exact(&mut count_bytes)?;
        let symbol_count = u64::from_le_bytes(count_bytes);

        self.freqs = HashMap::with_capacity(symbol_count as usize);
        for _ in 0..symbol_count {
            let mut entry = [0u8; 9];
            input.read_exact(&mut entry)?;
            let freq: [u8; 8] = entry[1..9].try_into().unwrap();
            self.freqs.insert(entry[0], u64::from_le_bytes(freq));
        }
        self.tree = build_tree(&self.freqs);
        Ok(())
    }

    fn report(&self) {
        println!("distinct byte values: {}", self.freqs.len());
        if let Some(ref tree) = self.tree {
            println!("built huffman tree, total freq = {}", tree.freq());
        }
        if let Some(ref codes) = self.codes {
            println!("generated codes for {} bytes", codes.len());
        }
    }

    fn encode_chunk(&mut self, chunk: &[u8], output: &mut dyn Write) -> Result<()> {
        for &byte in chunk {
            let code = {
                let codes = self.codes.as_ref().expect("must finalize_feed before encoding");
                codes.get(&byte).expect("byte missing from code table").clone()
            };
            for bit in code {
                self.write_bit(bit);
            }
        }
        if !self.buf.is_empty() {
            output.write_all(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }

    fn finalize_encode(&mut self, output: &mut dyn Write) -> Result<()> {
        self.flush_bits();
        if !self.buf.is_empty() {
            output.write_all(&self.buf)?;
            self.buf.clear();
        }
        output.flush()?;
        Ok(())
    }

    fn decoder<'a>(&'a self, input: Box<dyn Read + 'a>, original_len: u64) -> Box<dyn Read + 'a> {
        let tree = self.tree.as_ref().expect("must call read_header or finalize_feed first");
        Box::new(HuffmanDecoder {
            tree,
            bits: BitReader::new(input),
            remaining: original_len,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_repeated_bytes() {
        let mut c = HuffmanCodec::new();
        c.feed(b"aaabbc").unwrap();
        assert_eq!(c.freqs.get(&b'a'), Some(&3));
        assert_eq!(c.freqs.get(&b'b'), Some(&2));
        assert_eq!(c.freqs.get(&b'c'), Some(&1));
    }

    #[test]
    fn builds_tree_from_multiple_symbols() {
        let mut c = HuffmanCodec::new();
        c.feed(b"aaabbc").unwrap();
        c.finalize_feed().unwrap();
        let tree = c.tree.as_ref().unwrap();
        assert_eq!(tree.freq(), 6);
    }

    #[test]
    fn builds_tree_from_single_symbol() {
        let mut c = HuffmanCodec::new();
        c.feed(b"xxxx").unwrap();
        c.finalize_feed().unwrap();
        let tree = c.tree.as_ref().unwrap();
        assert_eq!(tree.freq(), 4);
    }

    #[test]
    fn pops_smallest_frequency_first() {
        let mut heap = BinaryHeap::new();
        heap.push(HeapNode { node: Node::Leaf { byte: b'a', freq: 50 }, seq: 0 });
        heap.push(HeapNode { node: Node::Leaf { byte: b'b', freq: 10 }, seq: 1 });
        heap.push(HeapNode { node: Node::Leaf { byte: b'c', freq: 30 }, seq: 2 });

        assert_eq!(heap.pop().unwrap().node.freq(), 10);
        assert_eq!(heap.pop().unwrap().node.freq(), 30);
        assert_eq!(heap.pop().unwrap().node.freq(), 50);
    }

    #[test]
    fn round_trips_compress_decompress() {
        let data = b"this is a test of the huffman codec implementation";

        let mut codec = HuffmanCodec::new();
        codec.feed(data).unwrap();
        codec.finalize_feed().unwrap();

        let mut header = Vec::new();
        codec.write_header(&mut header).unwrap();

        let mut compressed = Vec::new();
        codec.encode_chunk(data, &mut compressed).unwrap();
        codec.finalize_encode(&mut compressed).unwrap();

        let mut dec = HuffmanCodec::new();
        dec.read_header(&mut std::io::Cursor::new(&header)).unwrap();

        let input = compressed.as_slice();
        let mut decoder = dec.decoder(Box::new(input), data.len() as u64);
        let mut output = Vec::new();
        decoder.read_to_end(&mut output).unwrap();

        assert_eq!(output, data);
    }
}
