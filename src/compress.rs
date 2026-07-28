use anyhow::Result;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crate::archive;
use crate::codec;
use crate::header::FileHeader;
use crate::meta;

pub fn run(input: &Path, output: &Path, method: codec::Method) -> Result<()> {
    let is_archive = input.is_dir();
    let input_meta = meta::read_meta(input)?;

    let plan = if is_archive { Some(archive::plan_archive(input)?) } else { None };

    let mut codec = codec::create(method);

    let mut original_len = 0u64;
    for_each_input_chunk(input, is_archive, plan.as_deref(), |chunk| {
        codec.feed(chunk)?;
        original_len += chunk.len() as u64;
        Ok(())
    })?;
    codec.finalize_feed()?;
    codec.report();

    let header = FileHeader::new(codec.method_id(), original_len, is_archive, input_meta);
    let out_file = std::fs::File::create(output)?;
    let mut out_writer = std::io::BufWriter::new(out_file);
    header.write_full(&mut out_writer, &*codec)?;

    if original_len == 0 {
        println!("input was empty, writing header-only output");
        out_writer.flush()?;
        return Ok(());
    }

    let pb = indicatif::ProgressBar::new(original_len);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    for_each_input_chunk(input, is_archive, plan.as_deref(), |chunk| {
        codec.encode_chunk(chunk, &mut out_writer)?;
        pb.inc(chunk.len() as u64);
        Ok(())
    })?;
    codec.finalize_encode(&mut out_writer)?;
    pb.finish_and_clear();

    let final_len = std::fs::metadata(output)?.len();
    println!(
        "wrote {} bytes to {:?} (original was {} bytes)",
        final_len, output, original_len,
    );
    let ratio = (final_len as f64) / (original_len as f64) * 100.0;
    println!("total output size is {:.1}% of original", ratio);

    Ok(())
}

fn for_each_input_chunk(
    input: &Path,
    is_archive: bool,
    plan: Option<&[PathBuf]>,
    mut f: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    if is_archive {
        let plan = plan.expect("archive plan required when is_archive is true");
        archive::stream_archive(input, plan, |chunk| f(chunk))?;
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
