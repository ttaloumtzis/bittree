use anyhow::Result;
use std::path::Path;

use crate::freq;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let data = std::fs::read(input)?; //try (?) to read the the input file to the data variable
    let freqs = freq::build_freq_table(&data); //build table with the data reference

    println!("read {} bytes from {:?}", data.len(), input);
    println!("distinct byte values: {}", freqs.len());

    // temporary: print the table so you can eyeball it
    let mut entries: Vec<_> = freqs.iter().collect();
    entries.sort_by_key(|&(_, &count)| std::cmp::Reverse(count));
    for (byte, count) in entries.iter().take(10) {
        println!("  byte {:>3} ({:?}): {}", byte, char::from(**byte), count);
    }

    println!("compress: {:?} -> {:?}", input, output);
    Ok(())
}
