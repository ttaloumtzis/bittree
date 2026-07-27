use anyhow::Result;
use std::path::Path;

use crate::archive;
use crate::bitio::BitReader;
use crate::header;
use crate::meta;
use crate::tree;
use crate::tree::Node;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let file_bytes = std::fs::read(input)?;
    println!("read {} bytes from {:?}", file_bytes.len(), input);

    // Legacy case: a completely empty .bitree file with no header at
    // all (older versions of compress.rs wrote this for empty input).
    // We can't recover the archive flag from it, so just write an
    // empty file.
    if file_bytes.is_empty() {
        println!("input was empty, writing empty output");
        std::fs::write(output, [])?;
        return Ok(());
    }

    // Step 1: parse the header out of the front of the file
    let (parsed_header, header_size) = header::read_header(&file_bytes)?;
    println!("original length was {} bytes", parsed_header.original_len);
    println!(
        "distinct byte values in header: {}",
        parsed_header.freqs.len()
    );
    println!("is folder archive: {}", parsed_header.is_archive);

    // Step 2: rebuild the decompressed bytes. Header-only files (0
    // original bytes - an empty file or an empty folder) have no tree
    // and no bitstream to decode at all.
    let output_bytes: Vec<u8> = if parsed_header.original_len == 0 {
        Vec::new()
    } else {
        // Rebuild the exact same Huffman tree from the frequencies.
        // Since build_tree is deterministic given the same freqs, this
        // reconstructs the identical tree that compress.rs used.
        let tree_root = tree::build_tree(&parsed_header.freqs);
        let tree_root = tree_root.expect("header had frequencies but tree build failed");

        // The compressed bitstream is everything after the header
        let compressed_bits = &file_bytes[header_size..];
        let mut reader = BitReader::new(compressed_bits);

        // Walk the tree bit by bit, emitting a byte each time we land
        // on a Leaf, until we've produced exactly original_len bytes.
        let mut bytes: Vec<u8> = Vec::new();

        while (bytes.len() as u64) < parsed_header.original_len {
            let mut current_node = &tree_root;

            loop {
                match current_node {
                    Node::Leaf { byte, .. } => {
                        bytes.push(*byte);
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

        bytes
    };

    println!("decoded {} bytes", output_bytes.len());

    // Step 3: write the result out. If the original input was a folder,
    // unpack the archive bytes back into real files/directories;
    // otherwise write it straight to `output` as a plain file.
    if parsed_header.is_archive {
        archive::extract_archive(&output_bytes, output)?;
        println!("extracted folder archive to {:?}", output);
    } else {
        std::fs::write(output, &output_bytes)?;
        // Restore the plain file's own metadata.
        meta::apply_meta(output, &parsed_header.meta)?;
        println!("wrote decompressed output to {:?}", output);
    }

    Ok(())
}
