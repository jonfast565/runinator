//! turning a package directory into the exact same bytes every time.
//!
//! the archive is content-addressed, so determinism is not a nicety: if two machines zip one tree
//! into two digests, republishing an unchanged package uploads and stores a second copy, and
//! "this workflow is pinned to these bytes" stops meaning anything. four things are therefore fixed
//! rather than taken from the filesystem — entry order, timestamps, permissions, and the
//! compression method — so the digest is a function of the file contents and their paths alone.

use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

use crate::errors::{PackError, Result};

use super::glob;

/// paths never archived, whatever the manifest says.
///
/// these are build output, version-control state, and editor droppings: including them would make
/// the digest depend on whether the publisher had run a build, which is exactly the dependence the
/// fixed timestamps and ordering exist to remove.
pub const DEFAULT_EXCLUDES: &[&str] = &[
    ".git/",
    ".hg/",
    ".svn/",
    "target/",
    "node_modules/",
    ".venv/",
    "venv/",
    "**/__pycache__/",
    "**/*.pyc",
    "**/.DS_Store",
    "**/.idea/",
    "**/.vscode/",
];

/// the zip epoch, used as every entry's timestamp.
///
/// dos time cannot represent anything earlier, and using the file's real mtime would make a fresh
/// `git clone` produce a different digest from the tree it was cloned from.
const FIXED_TIMESTAMP: (u16, u8, u8, u8, u8, u8) = (1980, 1, 1, 0, 0, 0);

/// the permission bits every entry is written with.
///
/// the executable bit is deliberately not preserved: it does not survive a windows checkout, so
/// honouring it would make the digest platform-dependent. a runtime invokes a handler through its
/// interpreter, never by exec'ing a file out of the package.
const FIXED_MODE: u32 = 0o644;

/// a package archived into bytes, with the digest those bytes address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FunctionArchive {
    /// `sha256:<hex>` of [`Self::bytes`].
    pub digest: String,
    pub bytes: Vec<u8>,
    /// the archived paths, relative and `/`-separated, in the order they were written.
    pub files: Vec<String>,
}

impl FunctionArchive {
    pub fn size_bytes(&self) -> i64 {
        self.bytes.len() as i64
    }
}

/// archive a package directory, excluding [`DEFAULT_EXCLUDES`] plus any extra patterns.
pub fn archive_directory(directory: &Path, exclude: &[String]) -> Result<FunctionArchive> {
    if !directory.is_dir() {
        return Err(PackError::source(format!(
            "{} is not a directory",
            directory.display()
        )));
    }
    let mut files = Vec::new();
    collect(directory, directory, exclude, &mut files)?;
    if files.is_empty() {
        return Err(PackError::source(format!(
            "{} has no files to archive",
            directory.display()
        )));
    }
    // sorting is what makes the walk order irrelevant; readdir order is filesystem-dependent.
    files.sort();

    let mut buffer = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(Cursor::new(&mut buffer));
        let timestamp = zip::DateTime::from_date_and_time(
            FIXED_TIMESTAMP.0,
            FIXED_TIMESTAMP.1,
            FIXED_TIMESTAMP.2,
            FIXED_TIMESTAMP.3,
            FIXED_TIMESTAMP.4,
            FIXED_TIMESTAMP.5,
        )
        .map_err(|err| PackError::source(format!("invalid fixed archive timestamp: {err}")))?;
        // stored keeps the zip backend dependency-free (it is built without compression features)
        // and makes the output a pure function of the input with no compressor version in the mix.
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored)
            .last_modified_time(timestamp)
            .unix_permissions(FIXED_MODE);
        // directory entries are omitted: they carry no content, and their presence would be one
        // more thing the walk could vary.
        for relative in &files {
            let bytes = std::fs::read(directory.join(relative))?;
            zip.start_file(relative.as_str(), options)
                .map_err(|err| PackError::source(format!("failed to archive {relative}: {err}")))?;
            zip.write_all(&bytes)?;
        }
        zip.finish()
            .map_err(|err| PackError::source(format!("failed to finish archive: {err}")))?;
    }

    let digest = runinator_models::functions::digest_from_hex(&hex(&Sha256::digest(&buffer)));
    Ok(FunctionArchive {
        digest,
        bytes: buffer,
        files,
    })
}

/// list what an archive of this directory would contain, without building it.
pub fn archive_contents(directory: &Path, exclude: &[String]) -> Result<Vec<String>> {
    let mut files = Vec::new();
    collect(directory, directory, exclude, &mut files)?;
    files.sort();
    Ok(files)
}

// depth-first walk collecting relative paths, skipping excluded entries and symlinks.
fn collect(
    root: &Path,
    directory: &Path,
    exclude: &[String],
    files: &mut Vec<String>,
) -> Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(directory)?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .collect();
    entries.sort();

    for path in entries {
        let Some(relative) = relative_path(root, &path) else {
            continue;
        };
        if is_excluded(&relative, exclude) {
            continue;
        }
        // symlink_metadata rather than metadata: a symlink is skipped rather than followed, since a
        // link out of the package would archive bytes the publisher never meant to ship.
        let metadata = std::fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            collect(root, &path, exclude, files)?;
            continue;
        }
        if metadata.is_file() {
            files.push(relative);
        }
    }
    Ok(())
}

// the `/`-separated path of `path` under `root`, or None if it escapes.
fn relative_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let parts: Vec<String> = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect();
    Some(parts.join("/"))
}

/// true when a relative path is excluded by the defaults or by an extra pattern.
pub fn is_excluded(relative: &str, exclude: &[String]) -> bool {
    DEFAULT_EXCLUDES
        .iter()
        .any(|pattern| glob::matches(pattern, relative))
        || exclude
            .iter()
            .any(|pattern| glob::matches(pattern, relative))
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
}
