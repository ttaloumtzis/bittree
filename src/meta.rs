use anyhow::{Context, Result, bail};
use std::io::Read;
use std::path::Path;
use std::time::UNIX_EPOCH;

pub struct FileMeta {
    pub modified_secs: u64,
    pub permissions: u32,
}

pub fn read_meta(path: &Path) -> Result<FileMeta> {
    let metadata = std::fs::metadata(path)?;

    let modified_secs = metadata
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
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

pub fn write_meta_bytes(out: &mut Vec<u8>, meta: &FileMeta) {
    out.extend_from_slice(&meta.modified_secs.to_le_bytes());
    out.extend_from_slice(&meta.permissions.to_le_bytes());
}

pub fn read_meta_bytes(data: &[u8], pos: &mut usize) -> Result<FileMeta> {
    if *pos + 12 > data.len() {
        bail!("truncated metadata");
    }

    let secs_bytes: [u8; 8] = data[*pos..*pos + 8].try_into().unwrap();
    let modified_secs = u64::from_le_bytes(secs_bytes);
    *pos += 8;

    let perm_bytes: [u8; 4] = data[*pos..*pos + 4].try_into().unwrap();
    let permissions = u32::from_le_bytes(perm_bytes);
    *pos += 4;

    Ok(FileMeta {
        modified_secs,
        permissions,
    })
}

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

pub fn read_meta_from_reader<R: Read>(reader: &mut R) -> Result<FileMeta> {
    let mut secs_bytes = [0u8; 8];
    reader
        .read_exact(&mut secs_bytes)
        .context("failed to read modified timestamp from metadata")?;
    let modified_secs = u64::from_le_bytes(secs_bytes);

    let mut perm_bytes = [0u8; 4];
    reader
        .read_exact(&mut perm_bytes)
        .context("failed to read permissions from metadata")?;
    let permissions = u32::from_le_bytes(perm_bytes);

    Ok(FileMeta {
        modified_secs,
        permissions,
    })
}
