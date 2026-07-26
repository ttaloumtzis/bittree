use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "bitree", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    // Compress command definition
    Compress {
        // Input file to compress
        input: PathBuf,

        // Output file (defaults to <input.bitree>)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    // Decompress a .bitree file
    Decompress {
        // Input .bitree file
        input: PathBuf,

        // Output file (defaults ro striping .bitree)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}
