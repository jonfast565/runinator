//! filesystem layout and the atomic write primitives the backend is built from.
//!
//! a bucket is three sibling trees so a listing never has to distinguish data from sidecars:
//!
//! ```text
//! <root>/<bucket>/data/<key>            the bytes
//! <root>/<bucket>/meta/<key>.json       the descriptor
//! <root>/<bucket>/uploads/<id>/         multipart staging
//! <root>/<bucket>/.tmp/                 partial writes awaiting rename
//! ```
//!
//! keys are mirrored rather than hashed so `list` is a directory walk. two divergences from s3
//! follow from that and are accepted deliberately: a case-insensitive filesystem (a macos dev box)
//! collapses keys differing only in case, and a key cannot be both an object and a prefix of another
//! object (`a` and `a/b`), which the filesystem refuses. both are reported as errors rather than
//! silently resolved.

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::errors::BlobError;
use crate::key::ObjectKey;

pub(super) const DATA_DIR: &str = "data";
pub(super) const META_DIR: &str = "meta";
pub(super) const UPLOADS_DIR: &str = "uploads";
pub(super) const TMP_DIR: &str = ".tmp";
/// written when a bucket is created so an empty bucket is distinguishable from an absent one.
pub(super) const BUCKET_MARKER: &str = ".bucket";

/// the on-disk locations for one bucket.
#[derive(Clone)]
pub(super) struct BucketPaths {
    pub(super) root: PathBuf,
}

impl BucketPaths {
    pub(super) fn new(root: &Path, bucket: &str) -> Self {
        Self {
            root: root.join(bucket),
        }
    }

    pub(super) fn data(&self, key: &ObjectKey) -> PathBuf {
        self.root.join(DATA_DIR).join(key.as_str())
    }

    pub(super) fn meta(&self, key: &ObjectKey) -> PathBuf {
        self.root
            .join(META_DIR)
            .join(format!("{}.json", key.as_str()))
    }

    pub(super) fn data_root(&self) -> PathBuf {
        self.root.join(DATA_DIR)
    }

    pub(super) fn meta_root(&self) -> PathBuf {
        self.root.join(META_DIR)
    }

    pub(super) fn upload(&self, upload_id: &str) -> PathBuf {
        self.root.join(UPLOADS_DIR).join(upload_id)
    }

    pub(super) fn tmp_root(&self) -> PathBuf {
        self.root.join(TMP_DIR)
    }

    pub(super) fn marker(&self) -> PathBuf {
        self.root.join(BUCKET_MARKER)
    }
}

/// create a path's parent directory, translating the "a parent is itself an object" collision into
/// a legible error instead of a bare io failure.
pub(super) async fn ensure_parent(path: &Path) -> Result<(), BlobError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    match fs::create_dir_all(parent).await {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotADirectory => {
            Err(BlobError::InvalidKey(format!(
                "a prefix of this key already exists as an object: {}",
                parent.display()
            )))
        }
        Err(err) => Err(BlobError::Io(format!(
            "creating {}: {err}",
            parent.display()
        ))),
    }
}

/// write bytes to a temporary file inside the bucket, returning its path. the caller commits it with
/// [`commit_replace`] or [`commit_exclusive`]; both are renames within one filesystem, so no partial
/// object is ever visible under its final name.
pub(super) async fn stage(
    paths: &BucketPaths,
    name: &str,
    bytes: &[u8],
) -> Result<PathBuf, BlobError> {
    let tmp_root = paths.tmp_root();
    fs::create_dir_all(&tmp_root)
        .await
        .map_err(|err| BlobError::Io(format!("creating {}: {err}", tmp_root.display())))?;
    let tmp = tmp_root.join(name);
    fs::write(&tmp, bytes)
        .await
        .map_err(|err| BlobError::Io(format!("writing {}: {err}", tmp.display())))?;
    Ok(tmp)
}

/// move a staged file into place, overwriting whatever was there.
pub(super) async fn commit_replace(tmp: &Path, final_path: &Path) -> Result<(), BlobError> {
    ensure_parent(final_path).await?;
    fs::rename(tmp, final_path).await.map_err(|err| {
        BlobError::Io(format!(
            "committing {} to {}: {err}",
            tmp.display(),
            final_path.display()
        ))
    })
}

/// move a staged file into place only if nothing is there.
///
/// a hard link is what makes this exclusive: `rename` would happily clobber, while `link` fails with
/// `AlreadyExists` and does so atomically, which is exactly the guarantee `If-None-Match: *` needs
/// for a content-addressed write-once store.
pub(super) async fn commit_exclusive(tmp: &Path, final_path: &Path) -> Result<(), BlobError> {
    ensure_parent(final_path).await?;
    let (source, destination) = (tmp.to_path_buf(), final_path.to_path_buf());
    let linked = tokio::task::spawn_blocking(move || std::fs::hard_link(&source, &destination))
        .await
        .map_err(|err| BlobError::Io(format!("link task failed: {err}")))?;
    let _ = fs::remove_file(tmp).await;
    match linked {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            Err(BlobError::AlreadyExists(final_path.display().to_string()))
        }
        Err(err) => Err(BlobError::Io(format!(
            "committing {}: {err}",
            final_path.display()
        ))),
    }
}

/// map an io error on a read path, turning a missing file into the domain's not-found.
pub(super) fn read_error(path: &Path, key: &str, err: std::io::Error) -> BlobError {
    if err.kind() == std::io::ErrorKind::NotFound {
        return BlobError::NotFound(key.to_string());
    }
    BlobError::Io(format!("reading {}: {err}", path.display()))
}
