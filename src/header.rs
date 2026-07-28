use anyhow::{Context, Result, bail};
use std::io::Read;

use crate::codec::{self, Codec};
use crate::meta::{FileMeta, read_meta_from_reader, write_meta_bytes};

const MAGIC: [u8; 5] = *b"BTRE1";

pub struct FileHeader {
    pub method_id: u8,
    pub is_archive: bool,
    pub meta: FileMeta,
    pub original_len: u64,
}

impl FileHeader {
    pub fn new(method_id: u8, original_len: u64, is_archive: bool, meta: FileMeta) -> Self {
        FileHeader { method_id, is_archive, meta, original_len }
    }

    pub fn write_full<W: std::io::Write>(&self, output: &mut W, codec: &dyn Codec) -> Result<()> {
        self.write(output)?;
        codec.write_header(output)
    }

    pub fn read_full<R: Read>(input: &mut R) -> Result<(Self, Box<dyn Codec>)> {
        let header = Self::read(input)?;
        let mut codec = codec::by_id(header.method_id);
        codec.read_header(input)?;
        Ok((header, codec))
    }

    pub fn write<W: std::io::Write>(&self, output: &mut W) -> Result<()> {
        let mut buf = Vec::with_capacity(5 + 1 + 1 + 12 + 8);
        buf.extend_from_slice(&MAGIC);
        buf.push(self.method_id);
        buf.push(self.is_archive as u8);
        write_meta_bytes(&mut buf, &self.meta);
        buf.extend_from_slice(&self.original_len.to_le_bytes());
        output.write_all(&buf)?;
        Ok(())
    }

    pub fn read<R: Read>(input: &mut R) -> Result<Self> {
        let mut magic = [0u8; 5];
        input.read_exact(&mut magic).context("data too short for magic")?;
        if magic != MAGIC {
            bail!("not a valid bitree file");
        }

        let mut method_id = [0u8; 1];
        input.read_exact(&mut method_id).context("truncated header: missing method id")?;

        let mut flag = [0u8; 1];
        input.read_exact(&mut flag).context("truncated header: missing archive flag")?;

        let meta = read_meta_from_reader(input).context("truncated header: missing metadata")?;

        let mut len_bytes = [0u8; 8];
        input.read_exact(&mut len_bytes).context("truncated header: missing original length")?;

        Ok(FileHeader {
            method_id: method_id[0],
            is_archive: flag[0] == 1,
            meta,
            original_len: u64::from_le_bytes(len_bytes),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meta::FileMeta;

    fn dummy_meta() -> FileMeta {
        FileMeta { modified_secs: 1_700_000_000, permissions: 0o644 }
    }

    #[test]
    fn round_trips_a_small_header() {
        let meta = dummy_meta();
        let h = FileHeader::new(0, 8, false, meta);
        let mut bytes = Vec::new();
        h.write(&mut bytes).unwrap();

        let parsed = FileHeader::read(&mut std::io::Cursor::new(&bytes)).unwrap();
        assert_eq!(parsed.original_len, 8);
        assert_eq!(parsed.method_id, 0);
        assert!(!parsed.is_archive);
        assert_eq!(parsed.meta.modified_secs, 1_700_000_000);
    }

    #[test]
    fn round_trips_the_archive_flag() {
        let meta = dummy_meta();
        let h = FileHeader::new(0, 1, true, meta);
        let mut bytes = Vec::new();
        h.write(&mut bytes).unwrap();
        let parsed = FileHeader::read(&mut std::io::Cursor::new(&bytes)).unwrap();
        assert!(parsed.is_archive);
    }

    #[test]
    fn rejects_truncated_data() {
        let short = vec![1, 2, 3];
        assert!(FileHeader::read(&mut std::io::Cursor::new(&short)).is_err());
    }
}
