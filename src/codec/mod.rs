use std::io::{Read, Write};
use anyhow::Result;

pub mod huffman;
pub mod lz77;
pub mod deflate;

#[derive(clap::ValueEnum, Clone, Debug)]
pub enum Method {
    Huffman,
    Lz77,
    Deflate,
}

pub trait Codec {
    fn method_id(&self) -> u8;
    fn name(&self) -> &str;
    fn feed(&mut self, chunk: &[u8]) -> Result<()>;
    fn finalize_feed(&mut self) -> Result<()>;
    fn write_header(&self, output: &mut dyn Write) -> Result<()>;
    fn read_header(&mut self, input: &mut dyn Read) -> Result<()>;
    fn report(&self);
    fn encode_chunk(&mut self, chunk: &[u8], output: &mut dyn Write) -> Result<()>;
    fn finalize_encode(&mut self, output: &mut dyn Write) -> Result<()>;
    fn decoder<'a>(&'a self, input: Box<dyn Read + 'a>, original_len: u64) -> Box<dyn Read + 'a>;
}

pub fn create(method: Method) -> Box<dyn Codec> {
    match method {
        Method::Huffman => Box::new(huffman::HuffmanCodec::new()),
        Method::Lz77 => Box::new(lz77::Lz77Codec::new()),
        Method::Deflate => Box::new(deflate::DeflateCodec::new()),
    }
}

pub fn by_id(id: u8) -> Box<dyn Codec> {
    match id {
        0 => Box::new(huffman::HuffmanCodec::new()),
        1 => Box::new(lz77::Lz77Codec::new()),
        2 => Box::new(deflate::DeflateCodec::new()),
        _ => panic!("unknown codec id: {id}"),
    }
}
