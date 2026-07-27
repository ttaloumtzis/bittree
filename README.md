# Bitree

A file compressor with a pluggable `Codec` architecture, currently implementing
Huffman coding. Written in Rust.

Built as a learning project — no external compression crates, just `clap` for
CLI parsing, `anyhow` for error handling, and `indicatif` for progress bars.

## Features

- **Huffman coding** — compress/decompress any file losslessly
- **Folder archives** — compress entire directory trees, preserving structure
  and file metadata (timestamps, permissions)
- **Self-contained format** — compressed files carry their own frequency tables
  (and method ID), so no external metadata is needed to decompress
- **Streaming** — processes input in 64KB chunks regardless of file size
- **Progress bar** — shows real-time compression/decompression progress
- **Pluggable architecture** — adding a new algorithm means writing one file
  implementing the `Codec` trait

## Usage

```bash
# Compress a file (defaults to <input>.bitree)
bitree compress somefile.txt
bitree compress somefile.txt -o custom.bitree

# Compress a directory (recursive archive)
bitree compress somefolder/ -o folder.bitree

# Choose a compression method (huffman is default)
bitree compress somefile.txt --method huffman

# Decompress
bitree decompress somefile.txt.bitree
bitree decompress somefile.txt.bitree -o restored.txt
```

## How it works

The pipeline is split into a generic outer layer and a codec-specific inner
layer via the `Codec` trait:

1. **Outer layer** (`compress.rs`, `decompress.rs`) — handles file I/O,
   folder archive packing, progress bars, and dispatches to the chosen codec.
2. **Common header** (`header.rs`) — written before any codec data:
   `MAGIC | method_id | is_archive | meta | original_len`.
3. **Codec** (`codec/`) — each algorithm implements the `Codec` trait with a
   two-phase lifecycle: `feed()`/`finalize_feed()` for pre-processing, then
   `encode_chunk()`/`finalize_encode()` for output.

For Huffman specifically:
- `feed()` counts byte frequencies
- `finalize_feed()` builds the Huffman tree and code table
- `encode_chunk()` packs variable-length bit codes into bytes
- `decoder()` returns a `Read` that decompresses the bitstream on demand

## Compression results

Huffman coding exploits skewed byte frequency distributions:

| Input | Original size | Compressed size | Ratio |
|---|---|---|---|
| Plain English text (Shakespeare corpus) | 5.4 MB | 3.1 MB | 57.9% |
| Uncompressed 24-bit BMP photo | 4.5 MB | 4.2 MB | 92.8% |
| Already-compressed JPEG | — | — | ~no gain |

Text compresses well because letter frequencies are highly uneven (space and
`e` dominate). Already-compressed formats have near-uniform byte distributions,
so Huffman alone gains nothing.

## Project structure

```
src/
├── main.rs           # entry point, CLI dispatch
├── cli.rs            # clap argument/subcommand definitions (+ --method)
├── codec/
│   ├── mod.rs        # Codec trait + dispatch registry
│   └── huffman.rs    # HuffmanCodec (tree, codes, heap, freq — all private)
├── compress.rs       # generic compression pipeline (file + archive)
├── decompress.rs     # generic decompression pipeline (+ ProgressReader)
├── header.rs         # CommonHeader (method-agnostic)
├── archive.rs        # folder archive packing / extraction
├── meta.rs           # file metadata (timestamps, permissions)
└── bitio.rs          # BitReader / BitWriter utilities
```

## Adding a new algorithm

Create `src/codec/lz77.rs` implementing `Codec`, add it to the registry in
`codec/mod.rs`, and add the variant to the `Method` enum. That's it — the
pipeline handles the rest.

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
  *sequences* aren't exploited. DEFLATE (LZ77 + Huffman) would compress
  much further. The architecture is ready for it.
- Original filename isn't stored in the compressed file; renaming before
  decompress can lose the extension.

## License

MIT — see [LICENSE](LICENSE).
