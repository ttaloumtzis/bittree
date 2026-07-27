use anyhow::Result;
use std::collections::HashMap;
use std::io::Read;
use std::io::Write;
use std::path::Path;

use crate::archive;
use crate::bitio::BitWriter;
use crate::codes;
use crate::header;
use crate::meta;
use crate::tree;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let is_archive = input.is_dir();

    // Capture the input's own metadata before reading its bytes.
    let input_meta = meta::read_meta(input)?;

    let plan = if is_archive {
        Some(archive::plan_archive(input)?)
    } else {
        None
    };

    // Pass 1: count byte frequencies, streaming - never holds more
    // than one chunk (or one file's worth, for archives) at a time.
    let mut freqs: HashMap<u8, u64> = HashMap::new();
    let mut original_len: u64 = 0;
    let count_chunk = |chunk: &[u8]| -> Result<()> {
        // chunk is a small piece of the input's bytes (e.g. 64KB).
        // Count how many times each byte value shows up in it.
        for byte_value in chunk {
            let current_count = freqs.get(byte_value).unwrap_or(&0);
            let new_count = current_count + 1;
            freqs.insert(*byte_value, new_count);
        }

        // Keep a running total of how many bytes we've seen overall.
        original_len = original_len + chunk.len() as u64;

        Ok(())
    };

    for_each_input_chunk(input, is_archive, plan.as_deref(), count_chunk)?;
    println!("distinct byte values: {}", freqs.len());

    let pb = indicatif::ProgressBar::new(original_len);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

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
            let header_bytes = header::write_header(&freqs, original_len, is_archive, &input_meta);
            std::fs::write(output, &header_bytes)?;
            return Ok(());
        }
    };
    println!("built huffman tree, total freq = {}", tree_root.freq());

    let code_table = codes::build_codes(&tree_root);
    println!("generated codes for {} bytes", code_table.len());

    // Build the header: magic + archive flag + frequency table + original length
    let header_bytes = header::write_header(&freqs, original_len, is_archive, &input_meta);

    // Pass 2: Stream-encode directly to output on top of the header - no full buffer in memory
    let out_file = std::fs::File::create(output)?;
    let mut out_writer = std::io::BufWriter::new(out_file);
    out_writer.write_all(&header_bytes)?;

    let mut bit_writer = BitWriter::new(out_writer);

    let encode_chunk = |chunk: &[u8]| -> Result<()> {
        for &byte in chunk {
            let code = code_table
                .get(&byte)
                .expect("Encountered a byte missing from the code table");

            bit_writer.write_bits(code)?;
        }
        pb.inc(chunk.len() as u64);
        Ok(())
    };

    for_each_input_chunk(input, is_archive, plan.as_deref(), encode_chunk)?;

    // finish() flushes any partial (padded) byte and hands back the
    // inner BufWriter; flush that too so every byte is actually on
    // disk before we read the file's size back below.

    let mut out_writer = bit_writer.finish()?;
    out_writer.flush()?;
    drop(out_writer);

    pb.finish_and_clear();

    let final_len = std::fs::metadata(output)?.len();

    println!(
        "wrote {} bytes to {:?} (original was {} bytes, header was {} bytes)",
        final_len,
        output,
        original_len,
        header_bytes.len()
    );

    let ratio = (final_len as f64) / (original_len as f64) * 100.0;
    println!(
        "total output size is {:.1}% of original (including header)",
        ratio
    );

    Ok(())
}

/// Feed the input's bytes to `f`, one chunk at a time, without ever
/// holding the whole input in memory. Works for both a plain file
/// (read in fixed-size chunks) and a directory (streamed via
/// archive::stream_archive using a pre-built plan).
fn for_each_input_chunk(
    input: &Path,
    is_archive: bool,
    plan: Option<&[archive::PlannedEntry]>,
    mut f: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    if is_archive {
        let plan = plan.expect("archive plan required when is_archive is true");
        archive::stream_archive(plan, |chunk| f(chunk))?;
    } else {
        let file = std::fs::File::open(input)?;
        let mut reader = std::io::BufReader::new(file);
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            f(&buf[..n])?;
        }
    }
    Ok(())
}
