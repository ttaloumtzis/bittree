use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};

/// Separate magic number from the .bitree header magic, so a corrupted
/// or mismatched file fails fast with a clear error instead of garbage
/// output.
const ARCHIVE_MAGIC: [u8; 6] = *b"BTAR01";

const ENTRY_FILE: u8 = 0;
const ENTRY_DIR: u8 = 1;

/// Recursively walk `root` and collect every file/dir path relative to
/// it, in a fixed sorted order so the same folder always produces the
/// same archive bytes (useful for tests / reproducibility).
fn collect_entries(root: &Path) -> Result<Vec<PathBuf>> {
    let mut entries = Vec::new();
    collect_entries_inner(root, root, &mut entries)?;
    entries.sort();
    Ok(entries)
}

fn collect_entries_inner(root: &Path, current: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    let read_dir =
        fs::read_dir(current).with_context(|| format!("reading directory {:?}", current))?;

    for entry in read_dir {
        let entry = entry?;
        let path = entry.path();
        let relative = path
            .strip_prefix(root)
            .with_context(|| format!("stripping prefix from {:?}", path))?
            .to_path_buf();

        if path.is_dir() {
            out.push(relative.clone());
            collect_entries_inner(root, &path, out)?;
        } else {
            out.push(relative);
        }
    }

    Ok(())
}

/// Archive paths are always stored with '/' separators (regardless of
/// the host OS), so a .bitree archive made on Windows can be extracted
/// correctly on Linux/macOS and vice versa.
fn to_archive_path_string(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn write_path(out: &mut Vec<u8>, path_bytes: &[u8]) {
    let len = path_bytes.len() as u16;
    for b in len.to_le_bytes() {
        out.push(b);
    }
    for b in path_bytes {
        out.push(*b);
    }
}

/// Pack an entire directory tree into a single in-memory byte stream.
///
/// This is the "tar" step, and it happens BEFORE Huffman compression -
/// the result is just a Vec<u8>, so compress.rs can treat it exactly
/// like the bytes of a regular file and run the existing pipeline
/// (freq table -> tree -> codes -> bit packing) over it unchanged.
///
/// Format:
///   MAGIC (6 bytes: "BTAR01")
///   entry_count (u32 LE)
///   for each entry:
///     kind (1 byte: 0 = file, 1 = dir)
///     path_len (u16 LE)
///     path bytes (UTF-8, '/' separated, relative to root)
///     [file only] content_len (u64 LE) + content bytes
pub fn build_archive(root: &Path) -> Result<Vec<u8>> {
    let entries = collect_entries(root)?;

    let mut out: Vec<u8> = Vec::new();
    for b in ARCHIVE_MAGIC {
        out.push(b);
    }

    let entry_count = entries.len() as u32;
    for b in entry_count.to_le_bytes() {
        out.push(b);
    }

    for relative in &entries {
        let full_path = root.join(relative);
        let path_str = to_archive_path_string(relative);
        let path_bytes = path_str.as_bytes();

        if full_path.is_dir() {
            out.push(ENTRY_DIR);
            write_path(&mut out, path_bytes);
        } else {
            out.push(ENTRY_FILE);
            write_path(&mut out, path_bytes);

            let content =
                fs::read(&full_path).with_context(|| format!("reading file {:?}", full_path))?;
            let content_len = content.len() as u64;
            for b in content_len.to_le_bytes() {
                out.push(b);
            }
            for b in &content {
                out.push(*b);
            }
        }
    }

    Ok(out)
}

/// Unpack an archive byte stream (as produced by build_archive) back
/// into real files and directories under `dest_root`.
pub fn extract_archive(data: &[u8], dest_root: &Path) -> Result<()> {
    // Even an "empty folder" archive still has magic + count (10 bytes),
    // so anything shorter than that can't be valid.
    if data.len() < 10 {
        bail!("archive data too short to be valid");
    }

    let magic = &data[0..6];
    if magic != ARCHIVE_MAGIC {
        bail!("not a valid bitree archive (bad magic number)");
    }
    let mut pos: usize = 6;

    let count_bytes = [data[pos], data[pos + 1], data[pos + 2], data[pos + 3]];
    let entry_count = u32::from_le_bytes(count_bytes);
    pos += 4;

    fs::create_dir_all(dest_root)
        .with_context(|| format!("creating output directory {:?}", dest_root))?;

    for _ in 0..entry_count {
        let kind = data[pos];
        pos += 1;

        let path_len = u16::from_le_bytes([data[pos], data[pos + 1]]) as usize;
        pos += 2;

        let path_str = std::str::from_utf8(&data[pos..pos + path_len])
            .context("archive entry path was not valid UTF-8")?
            .to_owned();
        pos += path_len;

        // Archive paths always use '/'; rebuild a native, OS-correct
        // PathBuf from that (so this works the same on Windows too).
        let relative: PathBuf = path_str.split('/').collect();
        let full_path = dest_root.join(&relative);

        if kind == ENTRY_DIR {
            fs::create_dir_all(&full_path)
                .with_context(|| format!("creating directory {:?}", full_path))?;
        } else {
            let content_len_bytes = [
                data[pos],
                data[pos + 1],
                data[pos + 2],
                data[pos + 3],
                data[pos + 4],
                data[pos + 5],
                data[pos + 6],
                data[pos + 7],
            ];
            let content_len = u64::from_le_bytes(content_len_bytes) as usize;
            pos += 8;

            let content = &data[pos..pos + content_len];
            pos += content_len;

            // A file's parent dir entry is usually archived separately,
            // but create_dir_all here is cheap insurance either way.
            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory {:?}", parent))?;
            }

            fs::write(&full_path, content)
                .with_context(|| format!("writing file {:?}", full_path))?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn round_trips_a_small_folder_tree() {
        let tmp = std::env::temp_dir().join(format!("bitree_archive_test_{}", std::process::id()));
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        let _ = fs::remove_dir_all(&tmp);

        fs::create_dir_all(src.join("sub")).unwrap();
        fs::write(src.join("a.txt"), b"hello world").unwrap();
        fs::write(src.join("sub/b.txt"), b"nested file").unwrap();
        fs::create_dir_all(src.join("empty_dir")).unwrap();

        let archive_bytes = build_archive(&src).unwrap();
        extract_archive(&archive_bytes, &dst).unwrap();

        assert_eq!(fs::read(dst.join("a.txt")).unwrap(), b"hello world");
        assert_eq!(fs::read(dst.join("sub/b.txt")).unwrap(), b"nested file");
        assert!(dst.join("empty_dir").is_dir());

        fs::remove_dir_all(&tmp).unwrap();
    }
}
