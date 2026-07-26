use anyhow::Result;
use std::path::Path;

pub fn run(input: &Path, output: &Path) -> Result<()> {
    println!("compress: {:?} -> {:?}", input, output);
    Ok(())
}
