use crate::{
    Id,
    error::{Result, corrupt},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::Path,
};
use tempfile::NamedTempFile;
pub fn hash_file(path: &Path) -> Result<Id> {
    let mut file = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = match file.read(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(Id(h.finalize().into()))
}
pub fn verify_file(path: &Path, expected: Id) -> Result<()> {
    if hash_file(path)? != expected {
        return Err(corrupt(format!("SHA-256 mismatch: {}", path.display())));
    }
    Ok(())
}
pub fn sync_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    // the reference repository rejects non-unix publication; portable callers only seal scratch packs.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
pub fn atomic_replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().ok_or_else(|| corrupt("path has no parent"))?;
    let mut tmp = NamedTempFile::new_in(parent)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| e.error)?;
    sync_dir(parent)
}
/// Install a complete, synced immutable blob. Existing blobs must verify.
pub fn install(tmp: NamedTempFile, dir: &Path, suffix: &str) -> Result<Id> {
    tmp.as_file().sync_all()?;
    let digest = hash_file(tmp.path())?;
    let path = dir.join(format!("{digest}{suffix}"));
    if path.exists() {
        verify_file(&path, digest)?;
        return Ok(digest);
    }
    match tmp.persist_noclobber(&path) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify_file(&path, digest)?
        }
        Err(e) => return Err(e.error.into()),
    }
    sync_dir(dir)?;
    Ok(digest)
}
pub fn read_limited(path: &Path, max: usize) -> Result<Vec<u8>> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    if size > max as u64 {
        return Err(corrupt("file exceeds metadata limit"));
    }
    let mut result = Vec::with_capacity(size as usize);
    Read::by_ref(&mut file)
        .take(max as u64 + 1)
        .read_to_end(&mut result)?;
    if result.len() > max {
        return Err(corrupt("growing/oversized metadata file"));
    }
    Ok(result)
}
pub fn read_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> Result<()> {
    while !buf.is_empty() {
        #[cfg(unix)]
        let result = std::os::unix::fs::FileExt::read_at(file, buf, offset);
        #[cfg(windows)]
        let result = std::os::windows::fs::FileExt::seek_read(file, buf, offset);
        #[cfg(not(any(unix, windows)))]
        let result: std::io::Result<usize> = Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "positional I/O unavailable",
        ));
        let n = match result {
            Ok(0) => return Err(corrupt("truncated on-disk record")),
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        };
        offset = offset
            .checked_add(n as u64)
            .ok_or_else(|| corrupt("disk offset overflow"))?;
        buf = &mut buf[n..];
    }
    Ok(())
}
pub fn make_dirs(root: &Path) -> Result<()> {
    for name in ["packs", "indexes", "catalogs", "tmp"] {
        fs::create_dir_all(root.join(name))?;
    }
    sync_dir(root)
}
