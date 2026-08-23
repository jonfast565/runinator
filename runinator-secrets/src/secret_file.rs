//! atomic persistence for issued credentials.

use std::io::{self, Write};
use std::path::Path;

/// write a credential through a sibling temporary file, fsync it, apply owner-only permissions on
/// unix, then atomically rename it into place. a crash leaves either the old complete credential or
/// the new complete credential, never a truncated key.
pub fn write_secret_file_atomic(path: &Path, secret: &[u8]) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "secret path has no parent"))?;
    std::fs::create_dir_all(parent)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("secret"),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        file.write_all(secret)?;
        file.sync_all()?;
        std::fs::rename(&temp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
#[path = "secret_file_tests.rs"]
mod tests;
