use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use crate::archive;
use crate::bitio::{BitReader, HuffmanByteReader};
use crate::header;
use crate::meta;
use crate::tree;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let input_len = std::fs::metadata(input)
        .with_context(|| format!("reading metadata of {:?}", input))?
        .len();

    // Legacy case: a completely empty .bitree file with no header at
    // all (older versions of compress.rs wrote this for empty input).
    if input_len == 0 {
        println!("input was empty, writing empty output");
        std::fs::write(output, [])?;
        return Ok(());
    }

    let file = File::open(input)
        .with_context(|| format!("opening {:?}", input))?;
    let mut buf_reader = BufReader::new(file);

    let header = header::read_header_from_reader(&mut buf_reader)
        .context("failed to read header")?;

    println!("original length was {} bytes", header.original_len);
    println!("distinct byte values in header: {}", header.freqs.len());
    println!("is folder archive: {}", header.is_archive);

    if header.original_len == 0 {
        // Header-only file: empty input was compressed — just write
        // an empty result (file or folder depending on the archive flag).
        println!("decoded 0 bytes");
        if header.is_archive {
            std::fs::create_dir_all(output)
                .with_context(|| format!("creating output directory {:?}", output))?;
        } else {
            std::fs::write(output, [])?;
            meta::apply_meta(output, &header.meta)?;
        }
        return Ok(());
    }

    let tree_root = tree::build_tree(&header.freqs)
        .expect("header had frequencies but tree build failed");

    let bit_reader = BitReader::new(buf_reader);
    let mut huffman_reader = HuffmanByteReader::new(
        &tree_root,
        bit_reader,
        header.original_len,
    );

    if header.is_archive {
        archive::extract_archive_from_reader(&mut huffman_reader, output)?;
        println!("extracted folder archive to {:?}", output);
    } else {
        let out_file = File::create(output)
            .with_context(|| format!("creating output file {:?}", output))?;
        let mut out_writer = BufWriter::new(out_file);

        let bytes_written = std::io::copy(&mut huffman_reader, &mut out_writer)
            .context("failed to decompress data")?;

        out_writer.flush()?;

        meta::apply_meta(output, &header.meta)?;
        println!("wrote {} decompressed bytes to {:?}", bytes_written, output);
    }

    Ok(())
}