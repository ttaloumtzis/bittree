use anyhow::Result;
use std::path::Path;

use crate::archive;
use crate::bitio::BitWriter;
use crate::codes;
use crate::freq;
use crate::header;
use crate::tree;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let is_archive = input.is_dir();

    // If the input is a folder, pack it into a single in-memory byte
    // stream first (archive.rs). From this point on `data` is just a
    // Vec<u8>, so everything below runs exactly as it did for a single
    // file - the Huffman pipeline doesn't need to know the difference.
    let data = if is_archive {
        println!("input {:?} is a directory, archiving it first", input);
        archive::build_archive(input)?
    } else {
        std::fs::read(input)?
    };
    println!("read {} bytes from {:?}", data.len(), input);

    let freqs = freq::build_freq_table(&data);
    println!("distinct byte values: {}", freqs.len());

    let original_len = data.len() as u64;

    let tree_root = tree::build_tree(&freqs);

    let tree_root = match tree_root {
        Some(root) => root,
        None => {
            // Empty input (empty file, or an empty/all-empty-dirs folder).
            // Still write a real header so decompress knows original_len
            // is 0 and, importantly, still knows the is_archive flag -
            // otherwise decompressing an empty archived folder would
            // silently produce a file instead of a folder.
            println!("input was empty, writing header-only output");
            let header_bytes = header::write_header(&freqs, original_len, is_archive);
            std::fs::write(output, &header_bytes)?;
            return Ok(());
        }
    };

    println!("built huffman tree, total freq = {}", tree_root.freq());

    let code_table = codes::build_codes(&tree_root);
    println!("generated codes for {} bytes", code_table.len());

    // Encode every byte of the original data using its code
    let mut writer = BitWriter::new();
    for byte in &data {
        let code = code_table.get(byte).unwrap();
        writer.write_bits(code);
    }
    let compressed_bits = writer.finish();

    // Build the header: magic + archive flag + frequency table + original length
    let header_bytes = header::write_header(&freqs, original_len, is_archive);

    // Final file = header bytes, followed by the compressed bitstream
    let mut final_bytes: Vec<u8> = Vec::new();
    for b in &header_bytes {
        final_bytes.push(*b);
    }
    for b in &compressed_bits {
        final_bytes.push(*b);
    }

    std::fs::write(output, &final_bytes)?;

    println!(
        "wrote {} bytes to {:?} (original was {} bytes, header was {} bytes)",
        final_bytes.len(),
        output,
        data.len(),
        header_bytes.len()
    );

    let ratio = (final_bytes.len() as f64) / (data.len() as f64) * 100.0;
    println!(
        "total output size is {:.1}% of original (including header)",
        ratio
    );

    Ok(())
}
