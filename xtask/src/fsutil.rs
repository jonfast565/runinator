use std::path::Path;

use anyhow::{Context, Result};

use crate::paths::ensure_dir;

/// recursively copies every file under `source` into `destination`, creating directories as needed.
pub fn copy_dir_recursive(source: &Path, destination: &Path) -> Result<()> {
    ensure_dir(destination)?;
    for entry in walkdir::WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let target = destination.join(relative);
        if entry.file_type().is_dir() {
            ensure_dir(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                ensure_dir(parent)?;
            }
            std::fs::copy(entry.path(), &target)
                .with_context(|| format!("failed to copy {}", entry.path().display()))?;
        }
    }
    Ok(())
}
