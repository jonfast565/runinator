//! filesystem layout and atomic commit primitives for the v2 backend.
//!
//! ```text
//! <root>/<bucket>/objects/<key>.blob    payload plus metadata footer
//! <root>/<bucket>/uploads/<id>/         multipart staging
//! <root>/<bucket>/.tmp/                 incomplete atomic writes
//! <root>/<bucket>/.bucket               layout-version marker
//! ```

use std::path::{Path, PathBuf};

use tokio::fs;

use crate::errors::BlobError;
use crate::key::ObjectKey;

pub(super) const OBJECTS_DIR: &str = "objects";
pub(super) const LEGACY_DATA_DIR: &str = "data";
pub(super) const LEGACY_META_DIR: &str = "meta";
pub(super) const UPLOADS_DIR: &str = "uploads";
pub(super) const TMP_DIR: &str = ".tmp";
pub(super) const BUCKET_MARKER: &str = ".bucket";
pub(super) const V2_MARKER: &[u8] = b"runinator-blob-v2\n";

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

    pub(super) fn object(&self, key: &ObjectKey) -> PathBuf {
        self.objects_root().join(format!("{key}.blob"))
    }

    pub(super) fn objects_root(&self) -> PathBuf {
        self.root.join(OBJECTS_DIR)
    }

    pub(super) fn legacy_data(&self, key: &ObjectKey) -> PathBuf {
        self.root.join(LEGACY_DATA_DIR).join(key.as_str())
    }

    pub(super) fn legacy_meta(&self, key: &ObjectKey) -> PathBuf {
        self.root
            .join(LEGACY_META_DIR)
            .join(format!("{}.json", key.as_str()))
    }

    pub(super) fn legacy_data_root(&self) -> PathBuf {
        self.root.join(LEGACY_DATA_DIR)
    }

    pub(super) fn legacy_meta_root(&self) -> PathBuf {
        self.root.join(LEGACY_META_DIR)
    }

    pub(super) fn upload(&self, upload_id: &str) -> PathBuf {
        self.root.join(UPLOADS_DIR).join(upload_id)
    }

    pub(super) fn uploads_root(&self) -> PathBuf {
        self.root.join(UPLOADS_DIR)
    }

    pub(super) fn tmp_root(&self) -> PathBuf {
        self.root.join(TMP_DIR)
    }

    pub(super) fn marker(&self) -> PathBuf {
        self.root.join(BUCKET_MARKER)
    }
}

pub(super) async fn ensure_parent(path: &Path) -> Result<(), BlobError> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    fs::create_dir_all(parent)
        .await
        .map_err(|err| BlobError::Io(format!("creating {}: {err}", parent.display())))
}

pub(super) async fn create_staged(
    paths: &BucketPaths,
    name: &str,
) -> Result<(PathBuf, fs::File), BlobError> {
    let tmp_root = paths.tmp_root();
    fs::create_dir_all(&tmp_root)
        .await
        .map_err(|err| BlobError::Io(format!("creating {}: {err}", tmp_root.display())))?;
    let path = tmp_root.join(name);
    let file = fs::File::create(&path)
        .await
        .map_err(|err| BlobError::Io(format!("creating {}: {err}", path.display())))?;
    Ok((path, file))
}

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

pub(super) fn read_error(path: &Path, key: &str, err: std::io::Error) -> BlobError {
    if err.kind() == std::io::ErrorKind::NotFound {
        return BlobError::NotFound(key.to_string());
    }
    BlobError::Io(format!("reading {}: {err}", path.display()))
}
