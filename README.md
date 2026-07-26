# Bitree

A command-line file compressor implementing Huffman coding, written in Rust.

Built as a learning project to explore Rust fundamentals (ownership, enums,
traits, modules) by implementing a classic data compression algorithm from
scratch — no external compression crates, just `clap` for CLI parsing and
`anyhow` for error handling.

## Features

- Compress any file (text or binary) using Huffman coding
- Lossless decompression, verified byte-for-byte on real files
- Self-contained output format: the compressed file stores its own frequency
  table, so no external metadata is needed to decompress it
- Simple, readable module layout — one concept per file

## Usage

```bash
# Compress a file (defaults to <input>.bitree)
bitree compress somefile.txt
bitree compress somefile.txt -o custom_output.bitree

# Decompress a .bitree file
bitree decompress somefile.txt.bitree
bitree decompress somefile.txt.bitree -o restored.txt
```

## How it works

1. **Frequency counting** (`freq.rs`) — count how often each byte occurs in
   the input file.
2. **Tree building** (`tree.rs`, `heap.rs`) — build a Huffman tree by
   repeatedly merging the two least-frequent nodes using a min-heap, so
   frequent bytes end up with short codes and rare bytes end up with long
   codes.
3. **Code generation** (`codes.rs`) — walk the finished tree to produce a
   byte → bit-code lookup table.
4. **Bit packing** (`bitio.rs`) — pack the variable-length bit codes into
   real bytes (`BitWriter` for compression, `BitReader` for decompression).
5. **File format** (`header.rs`) — prepend a small header (magic number,
   frequency table, original length) to the compressed bitstream so the
   exact same tree can be rebuilt on decompression.

## Compression results

Huffman coding's effectiveness depends entirely on how skewed a file's byte
frequency distribution is — this is a direct, practical illustration of
Shannon entropy:

| Input | Original size | Compressed size | Ratio |
|---|---|---|---|
| Plain English text (Shakespeare corpus) | 5.4 MB | 3.1 MB | 57.9% |
| Uncompressed 24-bit BMP photo | 4.5 MB | 4.2 MB | 92.8% |
| Already-compressed JPEG | — | — | ~no gain expected |

Text compresses well because letter frequencies are highly uneven (space and
`e` dominate). Already-compressed formats (JPEG, PNG, ZIP, MP3) barely
compress further, since their byte streams are already close to maximum
entropy — there's no redundancy left for Huffman coding to exploit.

## Project structure

```
src/
├── main.rs          # entry point, CLI dispatch
├── cli.rs           # clap argument/subcommand definitions
├── freq.rs          # byte frequency counting
├── heap.rs          # min-heap wrapper (Ord impl) for tree building
├── tree.rs          # Huffman tree construction
├── codes.rs         # tree -> byte code table
├── bitio.rs          # BitWriter / BitReader bit-level packing
├── header.rs         # compressed file format (magic + freq table + length)
├── compress.rs       # compression pipeline
└── decompress.rs     # decompression pipeline
```

## Building

```bash
cargo build --release
```

## Testing

```bash
cargo test
```

## Known limitations

- Pure Huffman coding only — no dictionary/LZ-style matching, so repeated
  *sequences* of bytes (not just skewed single-byte frequencies) aren't
  exploited. Real-world tools like gzip/zip/PNG combine LZ77 with Huffman
  (as DEFLATE) for much stronger general-purpose compression.
- No streaming support — the whole input file is read into memory at once.
- Original filename/extension isn't stored in the compressed file; renaming
  the `.bitree` file before decompressing can lose the original extension.

## License

MIT — see [LICENSE](LICENSE).
