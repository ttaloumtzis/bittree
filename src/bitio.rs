// bitio.rs
use crate::tree::Node;
use std::io::{self, Read, Write};

/// Accumulates individual bits and packs them into real bytes.
///
/// Bits are filled left-to-right (most significant bit first) within
/// each byte, so if you write bits: 1,0,1,1,0,0,1,0
/// you get the byte: 0b10110010
pub struct BitWriter<W: Write> {
    out: W,
    current_byte: u8,
    bits_filled: u8,
}

impl<W: Write> BitWriter<W> {
    pub fn new(out: W) -> Self {
        BitWriter {
            out,
            current_byte: 0,
            bits_filled: 0,
        }
    }

    /// Write a single bit (true = 1, false = 0) into current_byte.
    pub fn write_bit(&mut self, bit: bool) -> io::Result<()> {
        if bit {
            // We fill left-to-right, so the FIRST bit written goes into
            // position 7 (the leftmost / most significant bit), the
            // SECOND bit goes into position 6, and so on down to position 0.
            //
            // bits_filled tracks how many bits we've already placed, so
            // "7 - bits_filled" gives the correct position for THIS bit.
            // e.g. bits_filled = 0 (first bit)  -> shift = 7 -> leftmost bit
            //      bits_filled = 7 (last bit)    -> shift = 0 -> rightmost bit
            let shift = 7 - self.bits_filled;

            // "1 << shift" creates a byte that is all zeros except for a
            // single 1 at position `shift`. Examples (shift -> result):
            //   shift=7 -> 0b10000000
            //   shift=3 -> 0b00001000
            //   shift=0 -> 0b00000001
            //
            // "|" is bitwise OR: it merges that single 1-bit into
            // current_byte without disturbing any bits already set there.
            // Example: if current_byte is 0b10100000 and we OR in
            // 0b00001000 (shift=3), the result is 0b10101000 - the new
            // bit gets added, everything else stays exactly as it was.
            self.current_byte |= 1 << shift;
        }
        // if bit is false, we do nothing at all - that position in
        // current_byte is already 0 by default, so there's nothing to set.

        self.bits_filled += 1;

        // Once we've placed 8 bits, current_byte is a complete, real byte.
        // Push it into the finished list and reset to start building
        // the next byte from scratch.
        if self.bits_filled == 8 {
            self.out.write_all(&[self.current_byte])?;
            self.current_byte = 0;
            self.bits_filled = 0;
        }
        Ok(())
    }

    /// Write a whole sequence of bits at once (e.g. one Huffman code).
    pub fn write_bits(&mut self, bits: &[bool]) -> io::Result<()> {
        for bit in bits {
            self.write_bit(*bit)?;
        }
        Ok(())
    }

    /// Call this when done writing all bits. If the last byte is only
    /// partially filled (bits_filled between 1 and 7), the unused
    /// positions on the right are left as 0 (padding) and the byte is
    /// pushed anyway - a partial byte still has to occupy a full byte
    /// on disk, there's no such thing as writing "3 bits" to a file.
    pub fn finish(mut self) -> io::Result<W> {
        if self.bits_filled > 0 {
            self.out.write_all(&[self.current_byte])?;
        }
        Ok(self.out)
    }
}

/// Reads individual bits back out of packed bytes, in the same
/// left-to-right (most significant bit first) order that BitWriter used.
pub struct BitReader<R: std::io::Read> {
    inner: std::io::BufReader<R>,
    current_byte: u8, // which byte in `bytes` we're currently reading from
    bit_pos: u8,      // which bit within that byte (0-7), 0 = leftmost
    exhausted: bool,
}

impl<R: std::io::Read> BitReader<R> {
    pub fn new(inner: R) -> Self {
        BitReader {
            inner: std::io::BufReader::new(inner),
            current_byte: 0,
            bit_pos: 8, // force a read on first call
            exhausted: false,
        }
    }

    /// Read the next single bit. Returns None if we've run out of bytes.
    pub fn read_bit(&mut self) -> io::Result<Option<bool>> {
        if self.exhausted {
            return Ok(None);
        }

        if self.bit_pos == 8 {
            let mut buf = [0u8; 1];
            match self.inner.read(&mut buf)? {
                0 => {
                    self.exhausted = true;
                    return Ok(None);
                }
                _ => {
                    self.current_byte = buf[0];
                    self.bit_pos = 0;
                }
            }
        }

        let shift = 7 - self.bit_pos;
        let bit = (self.current_byte >> shift) & 1 == 1;
        self.bit_pos += 1;
        Ok(Some(bit))
    }
}

/// Decodes a Huffman-compressed bitstream back into raw bytes, one
/// byte at a time, on demand - implemented as an ordinary `Read` so
/// callers (a plain file write, or archive extraction) can pull
/// decoded bytes in small chunks instead of requiring the whole
/// decompressed payload to be built up in memory first.
pub struct HuffmanByteReader<'a, R: Read> {
    tree_root: &'a Node,
    bit_reader: BitReader<R>,
    // How many decoded bytes are still owed before we've reproduced
    // the full original_len - needed because the tree alone can't
    // tell us where the (possibly zero-padded) bitstream ends.
    remaining: u64,
}

impl<'a, R: Read> HuffmanByteReader<'a, R> {
    pub fn new(tree_root: &'a Node, bit_reader: BitReader<R>, original_len: u64) -> Self {
        HuffmanByteReader {
            tree_root,
            bit_reader,
            remaining: original_len,
        }
    }
}

impl<'a, R: Read> Read for HuffmanByteReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut written = 0;

        while written < buf.len() && self.remaining > 0 {
            // Walk from the root, bit by bit, until we land on a leaf -
            // that leaf's byte is the next decoded byte.
            let mut current = self.tree_root;
            loop {
                match current {
                    Node::Leaf { byte, .. } => {
                        buf[written] = *byte;
                        written += 1;
                        self.remaining -= 1;
                        break;
                    }
                    Node::Internal { left, right, .. } => {
                        let bit = self.bit_reader.read_bit()?.ok_or_else(|| {
                            io::Error::new(
                                io::ErrorKind::UnexpectedEof,
                                "ran out of bits before reaching original_len",
                            )
                        })?;
                        if bit {
                            current = right;
                        } else {
                            current = left;
                        }
                    }
                }
            }
        }

        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_eight_bits_into_one_byte() {
        let mut writer = BitWriter::new(Vec::new());
        writer.write_bit(true).unwrap();
        writer.write_bit(false).unwrap();
        writer.write_bit(true).unwrap();
        writer.write_bit(true).unwrap();
        writer.write_bit(false).unwrap();
        writer.write_bit(false).unwrap();
        writer.write_bit(true).unwrap();
        writer.write_bit(false).unwrap();

        let bytes = writer.finish().unwrap();
        assert_eq!(bytes, vec![0b10110010]);
    }

    #[test]
    fn pads_incomplete_byte_with_zeros() {
        let mut writer = BitWriter::new(Vec::new());
        writer.write_bit(true).unwrap();
        writer.write_bit(true).unwrap();
        writer.write_bit(true).unwrap();

        let bytes = writer.finish().unwrap();
        assert_eq!(bytes, vec![0b11100000]);
    }

    #[test]
    fn reader_reverses_writer() {
        let mut writer = BitWriter::new(Vec::new());
        let bits = [
            true, false, true, true, false, false, true, false, true, true,
        ];
        writer.write_bits(&bits).unwrap();
        let packed = writer.finish().unwrap();

        let mut reader = BitReader::new(packed.as_slice());
        let mut read_back: Vec<bool> = Vec::new();
        for _ in 0..bits.len() {
            let bit = reader.read_bit().unwrap().unwrap();
            read_back.push(bit);
        }

        assert_eq!(read_back, bits);
    }
}
