use anyhow::{Context, Result};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::Path;

use crate::archive;
use crate::codec;
use crate::header::CommonHeader;
use crate::meta;

struct ProgressReader<'a, R: Read> {
    inner: R,
    pb: &'a indicatif::ProgressBar,
}

impl<'a, R: Read> Read for ProgressReader<'a, R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        self.pb.inc(n as u64);
        Ok(n)
    }
}

pub fn run(input: &Path, output: &Path) -> Result<()> {
    let input_len = std::fs::metadata(input)
        .with_context(|| format!("reading metadata of {:?}", input))?
        .len();

    if input_len == 0 {
        println!("input was empty, writing empty output");
        std::fs::write(output, [])?;
        return Ok(());
    }

    let file = std::fs::File::open(input).with_context(|| format!("opening {:?}", input))?;
    let mut buf_reader = BufReader::new(file);

    let common = CommonHeader::read(&mut buf_reader).context("failed to read header")?;

    let pb = indicatif::ProgressBar::new(common.original_len);
    pb.set_style(
        indicatif::ProgressStyle::default_bar()
            .template("[{elapsed_precise}] {bar:40.cyan/blue} {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("##-"),
    );

    println!("original length was {} bytes", common.original_len);
    println!("method id: {}", common.method_id);
    println!("is folder archive: {}", common.is_archive);

    if common.original_len == 0 {
        println!("decoded 0 bytes");
        if common.is_archive {
            std::fs::create_dir_all(output)
                .with_context(|| format!("creating output directory {:?}", output))?;
        } else {
            std::fs::write(output, [])?;
            meta::apply_meta(output, &common.meta)?;
        }
        return Ok(());
    }

    let mut codec = codec::by_id(common.method_id);
    codec.read_header(&mut buf_reader).context("failed to read codec header")?;

    let decoder = codec.decoder(Box::new(buf_reader), common.original_len);
    let mut progress = ProgressReader { inner: decoder, pb: &pb };

    if common.is_archive {
        archive::extract_archive_from_reader(&mut progress, output)?;
        pb.finish_and_clear();
        println!("extracted folder archive to {:?}", output);
    } else {
        let out_file = std::fs::File::create(output)
            .with_context(|| format!("creating output file {:?}", output))?;
        let mut out_writer = BufWriter::new(out_file);

        let mut buf = [0u8; 64 * 1024];
        let mut bytes_written = 0u64;
        loop {
            let n = progress.read(&mut buf)?;
            if n == 0 {
                break;
            }
            out_writer.write_all(&buf[..n])?;
            bytes_written += n as u64;
        }
        pb.finish_and_clear();

        out_writer.flush()?;
        meta::apply_meta(output, &common.meta)?;
        println!("wrote {} decompressed bytes to {:?}", bytes_written, output);
    }

    Ok(())
}
