/// Accumulates individual bits and packs them into real bytes.
///
/// Bits are filled left-to-right (most significant bit first) within
/// each byte, so if you write bits: 1,0,1,1,0,0,1,0
/// you get the byte: 0b10110010
pub struct BitWriter {
    bytes: Vec<u8>,   // completed, full bytes go here
    current_byte: u8, // the byte currently being built, one bit at a time
    bits_filled: u8,  // how many of current_byte's 8 slots are used (0-7)
}

impl BitWriter {
    pub fn new() -> Self {
        BitWriter {
            bytes: Vec::new(),
            current_byte: 0, // starts as 0b00000000 - every bit unset
            bits_filled: 0,
        }
    }

    /// Write a single bit (true = 1, false = 0) into current_byte.
    pub fn write_bit(&mut self, bit: bool) {
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
            self.current_byte = self.current_byte | (1 << shift);
        }
        // if bit is false, we do nothing at all - that position in
        // current_byte is already 0 by default, so there's nothing to set.

        self.bits_filled = self.bits_filled + 1;

        // Once we've placed 8 bits, current_byte is a complete, real byte.
        // Push it into the finished list and reset to start building
        // the next byte from scratch.
        if self.bits_filled == 8 {
            self.bytes.push(self.current_byte);
            self.current_byte = 0;
            self.bits_filled = 0;
        }
    }

    /// Write a whole sequence of bits at once (e.g. one Huffman code).
    pub fn write_bits(&mut self, bits: &[bool]) {
        for bit in bits {
            self.write_bit(*bit);
        }
    }

    /// Call this when done writing all bits. If the last byte is only
    /// partially filled (bits_filled between 1 and 7), the unused
    /// positions on the right are left as 0 (padding) and the byte is
    /// pushed anyway - a partial byte still has to occupy a full byte
    /// on disk, there's no such thing as writing "3 bits" to a file.
    pub fn finish(mut self) -> Vec<u8> {
        if self.bits_filled > 0 {
            self.bytes.push(self.current_byte);
        }
        self.bytes
    }
}

/// Reads individual bits back out of packed bytes, in the same
/// left-to-right (most significant bit first) order that BitWriter used.
pub struct BitReader<'a> {
    bytes: &'a [u8],
    byte_pos: usize, // which byte in `bytes` we're currently reading from
    bit_pos: u8,     // which bit within that byte (0-7), 0 = leftmost
}

impl<'a> BitReader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        BitReader {
            bytes,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    /// Read the next single bit. Returns None if we've run out of bytes.
    pub fn read_bit(&mut self) -> Option<bool> {
        if self.byte_pos >= self.bytes.len() {
            return None; // nothing left to read
        }

        let current_byte = self.bytes[self.byte_pos];

        // Same left-to-right convention as BitWriter: bit_pos=0 means
        // "leftmost bit", which is at shift position 7. bit_pos=7 means
        // rightmost bit, shift position 0.
        let shift = 7 - self.bit_pos;

        // Shift the byte so the bit we want ends up in position 0,
        // then mask with & 1 to isolate just that single bit, throwing
        // away everything else. Result is either 0 or 1.
        let bit_value = (current_byte >> shift) & 1;
        let bit = bit_value == 1;

        // advance to the next bit position, rolling over to the next
        // byte once we've read all 8 bits of the current one
        self.bit_pos = self.bit_pos + 1;
        if self.bit_pos == 8 {
            self.bit_pos = 0;
            self.byte_pos = self.byte_pos + 1;
        }

        Some(bit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_eight_bits_into_one_byte() {
        let mut writer = BitWriter::new();
        // Writing 1,0,1,1,0,0,1,0 left-to-right builds:
        //   position: 7 6 5 4 3 2 1 0
        //   bit:      1 0 1 1 0 0 1 0
        // which as a byte is 0b10110010 = 178 decimal
        writer.write_bit(true);
        writer.write_bit(false);
        writer.write_bit(true);
        writer.write_bit(true);
        writer.write_bit(false);
        writer.write_bit(false);
        writer.write_bit(true);
        writer.write_bit(false);

        let bytes = writer.finish();
        assert_eq!(bytes, vec![0b10110010]);
    }

    #[test]
    fn pads_incomplete_byte_with_zeros() {
        let mut writer = BitWriter::new();
        // Only 3 bits written: 1,1,1
        // Positions 7,6,5 get set; positions 4,3,2,1,0 stay 0 (padding)
        // Result: 0b11100000
        writer.write_bit(true);
        writer.write_bit(true);
        writer.write_bit(true);

        let bytes = writer.finish();
        assert_eq!(bytes, vec![0b11100000]);
    }

    #[test]
    fn reader_reverses_writer() {
        let mut writer = BitWriter::new();
        let bits = [
            true, false, true, true, false, false, true, false, true, true,
        ];
        writer.write_bits(&bits);
        let packed = writer.finish();

        let mut reader = BitReader::new(&packed);
        let mut read_back: Vec<bool> = Vec::new();
        for _ in 0..bits.len() {
            let bit = reader.read_bit().unwrap();
            read_back.push(bit);
        }

        assert_eq!(read_back, bits);
    }
}
