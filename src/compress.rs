use anyhow::Result;
use std::path::Path;

use crate::codes;
use crate::freq;
use crate::tree;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let data = std::fs::read(input)?; //try (?) to read the the input file to the data variable
    let freqs = freq::build_freq_table(&data); //build table with the data reference

    println!("read {} bytes from {:?}", data.len(), input);
    println!("distinct byte values: {}", freqs.len());

    // temporary: print the table so you can eyeball it
    // let mut entries: Vec<_> = freqs.iter().collect();
    // entries.sort_by_key(|&(_, &count)| std::cmp::Reverse(count));
    // for (byte, count) in entries.iter().take(10) {
    //     println!("  byte {:>3} ({:?}): {}", byte, char::from(**byte), count);
    // }

    let tree = tree::build_tree(&freqs);

    match tree {
        Some(root) => {
            println!("built huffman tree, total freq = {}", root.freq());

            let code_table = codes::build_codes(&root);
            println!("generated codes for {} bytes", code_table.len());

            // print a few codes so you can eyeball them
            for (byte, code) in code_table.iter().take(5) {
                let bits: String = code.iter().map(|&b| if b { '1' } else { '0' }).collect();
                println!("  byte {:>3} -> {}", byte, bits);
            }
        }
        None => {
            println!("input file was empty, nothing to compress");
        }
    }

    println!("compress: {:?} -> {:?}", input, output);
    Ok(())
}
