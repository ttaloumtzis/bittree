use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::codec;

#[derive(Parser)]
#[command(name = "bitree", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Compress {
        input: PathBuf,

        #[arg(short, long)]
        output: Option<PathBuf>,

        #[arg(short, long, value_enum, default_value_t = codec::Method::Huffman)]
        method: codec::Method,
    },

    Decompress {
        input: PathBuf,

        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}
