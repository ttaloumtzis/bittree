mod archive;
mod bitio;
mod cli;
mod codec;
mod compress;
mod decompress;
mod header;
mod meta;

use anyhow::Result;
use clap::Parser;
use cli::Command;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    match cli.command {
        Command::Compress { input, output, method } => {
            let output = output.unwrap_or_else(|| default_compress_output(&input));
            compress::run(&input, &output, method)?;
        }
        Command::Decompress { input, output } => {
            let output = output.unwrap_or_else(|| default_decompress_output(&input));
            decompress::run(&input, &output)?;
        }
    }

    Ok(())
}

fn default_compress_output(input: &Path) -> PathBuf {
    let mut p = input.as_os_str().to_owned();
    p.push(".bitree");
    PathBuf::from(p)
}

fn default_decompress_output(input: &Path) -> PathBuf {
    let mut p = input.to_path_buf();
    if p.extension().map_or(false, |ext| ext == "bitree") {
        p.set_extension("");
    } else {
        p.set_extension("out");
    }
    p
}
