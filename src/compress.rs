use anyhow::Result;
use std::path::Path;

use crate::bitio::BitWriter;
use crate::codes;
use crate::freq;
use crate::tree;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    // Step 1: read the whole file into memory as raw bytes
    let data = std::fs::read(input)?;
    println!("read {} bytes from {:?}", data.len(), input);

    // Step 2: count how often each byte occurs
    let freqs = freq::build_freq_table(&data);
    println!("distinct byte values: {}", freqs.len());

    // Step 3: build the Huffman tree from those frequencies
    let tree_root = tree::build_tree(&freqs);

    let tree_root = match tree_root {
        Some(root) => root,
        None => {
            // empty input file - nothing to compress, write an empty output
            println!("input file was empty, writing empty output");
            std::fs::write(output, [])?;
            return Ok(());
        }
    };

    println!("built huffman tree, total freq = {}", tree_root.freq());

    // Step 4: walk the tree to get a byte -> bitcode table
    let code_table = codes::build_codes(&tree_root);
    println!("generated codes for {} bytes", code_table.len());

    // Step 5: encode every byte of the original data using its code
    let mut writer = BitWriter::new();

    for byte in &data {
        // look up the code for this byte - it MUST exist, since the
        // code table was built from freqs, which was built from this
        // exact same data, so every byte we see here was already counted.
        let code = code_table.get(byte).unwrap();
        writer.write_bits(code);
    }

    let compressed_bytes = writer.finish();

    // Step 6: write the compressed bytes to the output file
    // NOTE: this is NOT decompressible yet - we haven't written a header
    // yet (no frequency table stored), so there's no way for decompress
    // to rebuild the same tree. That's the next piece we'll add.
    std::fs::write(output, &compressed_bytes)?;

    println!(
        "wrote {} compressed bytes to {:?} (original was {} bytes)",
        compressed_bytes.len(),
        output,
        data.len()
    );

    let ratio = (compressed_bytes.len() as f64) / (data.len() as f64) * 100.0;
    println!("compressed size is {:.1}% of original", ratio);

    Ok(())
}
