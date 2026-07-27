use anyhow::{Context, Result, bail};
use std::fs;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use crate::meta;

/// Bumped from BTAR01 -> BTAR02: entries now carry per-entry metadata
/// (mtime + permissions), so an old archive must fail fast with a clear
/// error instead of being misparsed against the new layout.
const ARCHIVE_MAGIC: [u8; 6] = *b"BTAR02";

const ENTRY_FILE: u8 = 0;
const ENTRY_DIR: u8 = 1;

/// One entry to be archived, with everything needed except the file's
/// content bytes (which are re-read from disk on demand, not stored here).
pub struct PlannedEntry {
    pub relative: PathBuf,
    pub full_path: PathBuf,
    pub kind: u8, // ENTRY_FILE or ENTRY_DIR
    pub meta: meta::FileMeta,
}

/// Walk `root` and build the (small) list of what will go into the
/// archive. No file content is touched here - just paths + metadata.
pub fn plan_archive(root: &Path) -> Result<Vec<PlannedEntry>> {
    let entries = collect_entries(root)?;

    let mut planned = Vec::with_capacity(entries.len());
    for relative in entries {
        let full_path = root.join(&relative);
        let kind = if full_path.is_dir() {
            ENTRY_DIR
        } else {
            ENTRY_FILE
        };
        let meta = meta::read_meta(&full_path)
            .with_context(|| format!("reading metadata for {:?}", full_path))?;

        planned.push(PlannedEntry {
            relative,
            full_path,
            kind,
            meta,
        });
    }

    Ok(planned)
}

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

/// Stream the archive's bytes to `sink`, one chunk at a time, without
/// ever holding the whole archive (or a whole file's contents) in
/// memory at once. Call this once per pass (e.g. once to count
/// frequencies, once to encode) - it re-reads file content from disk
/// each time, since PlannedEntry never stores content itself.
pub fn stream_archive(
    plan: &[PlannedEntry],
    mut sink: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    sink(&ARCHIVE_MAGIC)?;

    let entry_count = plan.len() as u32;
    sink(&entry_count.to_le_bytes())?;

    for entry in plan {
        let path_str = to_archive_path_string(&entry.relative);
        let path_bytes = path_str.as_bytes();

        sink(&[entry.kind])?;
        sink(&(path_bytes.len() as u16).to_le_bytes())?;
        sink(path_bytes)?;

        let mut meta_bytes = Vec::new();
        meta::write_meta_bytes(&mut meta_bytes, &entry.meta);
        sink(&meta_bytes)?;

        if entry.kind == ENTRY_FILE {
            let content_len = fs::metadata(&entry.full_path)?.len();
            sink(&content_len.to_le_bytes())?;

            // Stream the file itself in fixed-size chunks instead of
            // fs::read()-ing the whole thing into memory.
            let file = fs::File::open(&entry.full_path)
                .with_context(|| format!("opening file {:?}", entry.full_path))?;
            let mut reader = std::io::BufReader::new(file);
            let mut buf = [0u8; 64 * 1024];
            loop {
                let n = reader.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                sink(&buf[..n])?;
            }
        }
    }

    Ok(())
}

/// Pack an entire directory tree into a single in-memory byte stream.
///
/// This is the "tar" step, and it happens BEFORE Huffman compression -
/// the result is just a Vec<u8>, so compress.rs can treat it exactly
/// like the bytes of a regular file and run the existing pipeline
/// (freq table -> tree -> codes -> bit packing) over it unchanged.
///
/// Format (v2):
///   MAGIC (6 bytes: "BTAR02")
///   entry_count (u32 LE)
///   for each entry:
///     kind (1 byte: 0 = file, 1 = dir)
///     path_len (u16 LE)
///     path bytes (UTF-8, '/' separated, relative to root)
///     metadata (12 bytes: 8 mtime + 4 permissions)
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
        let file_meta = meta::read_meta(&full_path)
            .with_context(|| format!("reading metadata for {:?}", full_path))?;

        if full_path.is_dir() {
            out.push(ENTRY_DIR);
            write_path(&mut out, path_bytes);
            meta::write_meta_bytes(&mut out, &file_meta);
        } else {
            out.push(ENTRY_FILE);
            write_path(&mut out, path_bytes);
            meta::write_meta_bytes(&mut out, &file_meta);

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

/// Unpack an archive byte stream back into real files and directories
/// under `dest_root`.
///
/// Directory metadata is applied in a SECOND pass, after every file has
/// been written. Writing a file into a folder updates that folder's own
/// mtime, so applying a directory's saved mtime immediately after
/// creating it would just get overwritten by the files written into it
/// afterward.
pub fn extract_archive(data: &[u8], dest_root: &Path) -> Result<()> {
    extract_archive_from_reader(&mut std::io::Cursor::new(data), dest_root)
}

/// Streaming version of `extract_archive` — reads the archive format from
/// any `Read` stream without requiring the whole archive to be in memory.
/// Directory metadata is deferred to a second pass (same as
/// `extract_archive`) so file writes don't clobber directory mtimes.
pub fn extract_archive_from_reader<R: Read>(
    reader: &mut R,
    dest_root: &Path,
) -> Result<()> {
    let mut magic = [0u8; 6];
    reader
        .read_exact(&mut magic)
        .context("archive data too short to be valid")?;
    if magic != ARCHIVE_MAGIC {
        bail!("not a valid bitree archive (bad magic number)");
    }

    let mut count_buf = [0u8; 4];
    reader.read_exact(&mut count_buf)?;
    let entry_count = u32::from_le_bytes(count_buf);

    fs::create_dir_all(dest_root)
        .with_context(|| format!("creating output directory {:?}", dest_root))?;

    let mut pending_dir_meta: Vec<(PathBuf, meta::FileMeta)> = Vec::new();

    for _ in 0..entry_count {
        let mut kind_buf = [0u8; 1];
        reader.read_exact(&mut kind_buf)?;
        let kind = kind_buf[0];

        let mut path_len_buf = [0u8; 2];
        reader.read_exact(&mut path_len_buf)?;
        let path_len = u16::from_le_bytes(path_len_buf) as usize;

        let mut path_bytes = vec![0u8; path_len];
        reader.read_exact(&mut path_bytes)?;
        let path_str = String::from_utf8(path_bytes)
            .context("archive entry path was not valid UTF-8")?;

        let relative: PathBuf = path_str.split('/').collect();
        let full_path = dest_root.join(&relative);

        let file_meta = meta::read_meta_from_reader(reader)
            .with_context(|| format!("reading metadata for {:?}", full_path))?;

        if kind == ENTRY_DIR {
            fs::create_dir_all(&full_path)
                .with_context(|| format!("creating directory {:?}", full_path))?;
            pending_dir_meta.push((full_path, file_meta));
        } else {
            let mut content_len_buf = [0u8; 8];
            reader.read_exact(&mut content_len_buf)?;
            let content_len = u64::from_le_bytes(content_len_buf);

            if let Some(parent) = full_path.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating directory {:?}", parent))?;
            }

            let out_file = fs::File::create(&full_path)
                .with_context(|| format!("creating file {:?}", full_path))?;
            let mut writer = BufWriter::new(out_file);

            let mut transferred = 0u64;
            let mut buf = [0u8; 64 * 1024];
            while transferred < content_len {
                let remaining = content_len - transferred;
                let to_read = std::cmp::min(buf.len() as u64, remaining) as usize;
                let n = reader
                    .read(&mut buf[..to_read])
                    .context("unexpected EOF reading file content")?;
                if n == 0 {
                    bail!("unexpected EOF reading file content in {:?}", full_path);
                }
                writer
                    .write_all(&buf[..n])
                    .with_context(|| format!("writing file {:?}", full_path))?;
                transferred += n as u64;
            }
            writer.flush()?;

            meta::apply_meta(&full_path, &file_meta)
                .with_context(|| format!("applying metadata to {:?}", full_path))?;
        }
    }

    for (dir_path, dir_meta) in &pending_dir_meta {
        meta::apply_meta(dir_path, dir_meta)
            .with_context(|| format!("applying metadata to {:?}", dir_path))?;
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

    #[test]
    fn preserves_modification_time() {
        let tmp = std::env::temp_dir().join(format!("bitree_meta_test_{}", std::process::id()));
        let src = tmp.join("src");
        let dst = tmp.join("dst");
        let _ = fs::remove_dir_all(&tmp);

        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("a.txt"), b"content").unwrap();

        let original_meta = fs::metadata(src.join("a.txt")).unwrap();
        let original_modified = original_meta.modified().unwrap();

        let archive_bytes = build_archive(&src).unwrap();
        extract_archive(&archive_bytes, &dst).unwrap();

        let restored_meta = fs::metadata(dst.join("a.txt")).unwrap();
        let restored_modified = restored_meta.modified().unwrap();

        // Compare at second granularity, since that's all we store.
        let original_secs = original_modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let restored_secs = restored_modified
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        assert_eq!(original_secs, restored_secs);

        fs::remove_dir_all(&tmp).unwrap();
    }
}
