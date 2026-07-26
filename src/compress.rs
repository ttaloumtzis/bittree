use anyhow::Result;
use std::path::Path;

use crate::bitio::BitWriter;
use crate::codes;
use crate::freq;
use crate::header;
use crate::tree;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let data = std::fs::read(input)?;
    println!("read {} bytes from {:?}", data.len(), input);

    let freqs = freq::build_freq_table(&data);
    println!("distinct byte values: {}", freqs.len());

    let tree_root = tree::build_tree(&freqs);

    let tree_root = match tree_root {
        Some(root) => root,
        None => {
            println!("input file was empty, writing empty output");
            std::fs::write(output, [])?;
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

    // Build the header: frequency table + original length
    let original_len = data.len() as u64;
    let header_bytes = header::write_header(&freqs, original_len);

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
