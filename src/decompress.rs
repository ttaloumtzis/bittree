use anyhow::Result;
use std::path::Path;

use crate::bitio::BitReader;
use crate::header;
use crate::tree;
use crate::tree::Node;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let file_bytes = std::fs::read(input)?;
    println!("read {} bytes from {:?}", file_bytes.len(), input);

    // Handle the empty-file case (compress.rs writes an empty file
    // when the original input was empty, with no header at all)
    if file_bytes.is_empty() {
        println!("input was empty, writing empty output");
        std::fs::write(output, [])?;
        return Ok(());
    }

    // Step 1: parse the header out of the front of the file
    let (parsed_header, header_size) = header::read_header(&file_bytes);
    println!("original length was {} bytes", parsed_header.original_len);
    println!(
        "distinct byte values in header: {}",
        parsed_header.freqs.len()
    );

    // Step 2: rebuild the exact same Huffman tree from the frequencies.
    // Since build_tree is deterministic given the same freqs, this
    // reconstructs the identical tree that compress.rs used.
    let tree_root = tree::build_tree(&parsed_header.freqs);
    let tree_root = tree_root.expect("header had frequencies but tree build failed");

    // Step 3: the compressed bitstream is everything after the header
    let compressed_bits = &file_bytes[header_size..];
    let mut reader = BitReader::new(compressed_bits);

    // Step 4: walk the tree bit by bit, emitting a byte each time we
    // land on a Leaf, until we've produced exactly original_len bytes.
    let mut output_bytes: Vec<u8> = Vec::new();

    while (output_bytes.len() as u64) < parsed_header.original_len {
        let mut current_node = &tree_root;

        loop {
            match current_node {
                Node::Leaf { byte, .. } => {
                    output_bytes.push(*byte);
                    break;
                }
                Node::Internal { left, right, .. } => {
                    let bit = reader
                        .read_bit()
                        .expect("ran out of bits before reaching original_len");

                    if bit {
                        current_node = right;
                    } else {
                        current_node = left;
                    }
                }
            }
        }
    }

    println!("decoded {} bytes", output_bytes.len());

    std::fs::write(output, &output_bytes)?;
    println!("wrote decompressed output to {:?}", output);

    Ok(())
}
