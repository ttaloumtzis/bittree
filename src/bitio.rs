use std::io::{self, Read, Write};

/// Packs bits into bytes, MSB first.
pub struct BitWriter<W: Write> {
    out: W,
    current_byte: u8,
    bits_filled: u8,
}

impl<W: Write> BitWriter<W> {
    pub fn new(out: W) -> Self {
        BitWriter { out, current_byte: 0, bits_filled: 0 }
    }

    pub fn write_bit(&mut self, bit: bool) -> io::Result<()> {
        if bit {
            self.current_byte |= 1 << (7 - self.bits_filled);
        }
        self.bits_filled += 1;
        if self.bits_filled == 8 {
            self.out.write_all(&[self.current_byte])?;
            self.current_byte = 0;
            self.bits_filled = 0;
        }
        Ok(())
    }

    pub fn write_bits(&mut self, bits: &[bool]) -> io::Result<()> {
        for bit in bits {
            self.write_bit(*bit)?;
        }
        Ok(())
    }

    /// Flush the final byte (zero-padded if partial).
    pub fn finish(mut self) -> io::Result<W> {
        if self.bits_filled > 0 {
            self.out.write_all(&[self.current_byte])?;
        }
        Ok(self.out)
    }
}

/// Reads bits back from packed bytes (MSB first).
pub struct BitReader<R: Read> {
    inner: std::io::BufReader<R>,
    current_byte: u8,
    bit_pos: u8,
    exhausted: bool,
}

impl<R: Read> BitReader<R> {
    pub fn new(inner: R) -> Self {
        BitReader {
            inner: std::io::BufReader::new(inner),
            current_byte: 0,
            bit_pos: 8,
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

        let bit = (self.current_byte >> (7 - self.bit_pos)) & 1 == 1;
        self.bit_pos += 1;
        Ok(Some(bit))
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
