use anyhow::{Result, bail};
use std::path::Path;
use std::time::UNIX_EPOCH;

/// The metadata we preserve across compress/decompress: modification
/// time (seconds since Unix epoch) and Unix permission bits.
///
/// On non-Unix platforms, `permissions` is stored as 0 and simply not
/// applied on restore - there's no equivalent rwx bit model to map it
/// onto, so we don't pretend to.

pub struct FileMeta {
    pub modified_secs: u64,
    pub permissions: u32,
}

/// Read a path's current metadata off disk.
pub fn read_meta(path: &Path) -> Result<FileMeta> {
    let metadata = std::fs::metadata(path)?;

    let modified = metadata.modified()?;
    let modified_secs = modified
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default() // if somehow before 1970, just clamp to 0
        .as_secs();

    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode()
    };
    #[cfg(not(unix))]
    let permissions: u32 = 0;

    Ok(FileMeta {
        modified_secs,
        permissions,
    })
}

/// Serialize metadata as 12 bytes: 8 for mtime, 4 for permissions.
pub fn write_meta_bytes(out: &mut Vec<u8>, meta: &FileMeta) {
    for b in meta.modified_secs.to_le_bytes() {
        out.push(b);
    }
    for b in meta.permissions.to_le_bytes() {
        out.push(b);
    }
}

/// Read 12 metadata bytes starting at `*pos`, advancing `*pos` past them.
pub fn read_meta_bytes(data: &[u8], pos: &mut usize) -> Result<FileMeta> {
    if *pos + 12 > data.len() {
        bail!("truncated metadata");
    }

    let secs_bytes = [
        data[*pos],
        data[*pos + 1],
        data[*pos + 2],
        data[*pos + 3],
        data[*pos + 4],
        data[*pos + 5],
        data[*pos + 6],
        data[*pos + 7],
    ];
    let modified_secs = u64::from_le_bytes(secs_bytes);
    *pos += 8;

    let perm_bytes = [data[*pos], data[*pos + 1], data[*pos + 2], data[*pos + 3]];
    let permissions = u32::from_le_bytes(perm_bytes);
    *pos += 4;

    Ok(FileMeta {
        modified_secs,
        permissions,
    })
}

/// Apply saved metadata back onto a real path after writing/creating it.
pub fn apply_meta(path: &Path, meta: &FileMeta) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(meta.permissions);
        std::fs::set_permissions(path, perms)?;
    }

    let mtime = filetime::FileTime::from_unix_time(meta.modified_secs as i64, 0);
    filetime::set_file_mtime(path, mtime)?;

    Ok(())
}
