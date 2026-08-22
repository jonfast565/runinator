//! where artifact bytes live.
//!
//! Artifacts predate the object store, so `run_artifacts.uri` has two forms:
//! a `blob://` URI for new rows, or an absolute path from the replica that handled an old upload.
//! The old form could return 404 from another WS replica. Both forms are readable here; only the
//! first is written.

use std::path::PathBuf;
use std::sync::Arc;

use runinator_blob_core::{
    BlobError, BlobStore, ByteRange, ObjectKey, PutOptions, RUN_ARTIFACT_BUCKET, blob_uri,
    parse_blob_uri,
};
use runinator_models::errors::SendableError;
use tokio::io::AsyncRead;
use uuid::Uuid;

use crate::errors::{ARTIFACT_STORE_FAILED, ARTIFACT_UNREADABLE};

/// an artifact's bytes, however they are stored.
pub struct ArtifactContent {
    pub size_bytes: u64,
    pub body: Box<dyn AsyncRead + Send + Unpin>,
}

/// store artifact bytes and return the URI to record on the row.
///
/// The key is run-scoped and carries a UUID so two uploads of the same filename never collide.
pub async fn put_artifact(
    store: &Arc<dyn BlobStore>,
    run_id: Uuid,
    name: &str,
    mime_type: &str,
    bytes: &[u8],
) -> Result<String, SendableError> {
    let scope = format!("runs/{run_id}");
    let key = ObjectKey::parse(&format!(
        "{scope}/{}-{}",
        Uuid::new_v4().simple(),
        safe_name(name)
    ))
    .map_err(|err| ARTIFACT_STORE_FAILED.error(err))?;
    store
        .put(
            RUN_ARTIFACT_BUCKET,
            &key,
            bytes.to_vec(),
            PutOptions {
                content_type: Some(mime_type.to_string()),
                ..PutOptions::default()
            },
        )
        .await
        .map_err(|err| ARTIFACT_STORE_FAILED.error(err))?;
    Ok(blob_uri(RUN_ARTIFACT_BUCKET, &key))
}

/// open an artifact's bytes for streaming, from the object store or from the legacy local path.
pub async fn open_artifact(
    store: &Arc<dyn BlobStore>,
    uri: &str,
    range: Option<ByteRange>,
) -> Result<ArtifactContent, SendableError> {
    if let Some((bucket, key)) = parse_blob_uri(uri) {
        let reader = store
            .open(&bucket, &key, range)
            .await
            .map_err(|err| ARTIFACT_UNREADABLE.error(err))?;
        return Ok(ArtifactContent {
            size_bytes: reader.len(),
            body: reader.body,
        });
    }
    // pre-blob row: a path on the replica that served the upload. readable only from that replica,
    // which is the bug the object store exists to remove — but existing rows still have to work.
    let path = PathBuf::from(uri);
    let file = tokio::fs::File::open(&path)
        .await
        .map_err(|err| ARTIFACT_UNREADABLE.error(format!("{}: {err}", path.display())))?;
    let size_bytes = file
        .metadata()
        .await
        .map(|meta| meta.len())
        .map_err(|err| ARTIFACT_UNREADABLE.error(format!("{}: {err}", path.display())))?;
    Ok(ArtifactContent {
        size_bytes,
        body: Box::new(file),
    })
}

/// remove an artifact's bytes. best effort in both storage shapes: a missing object must not block
/// deleting the row that points at it, or the row becomes undeletable.
pub async fn delete_artifact_bytes(store: &Arc<dyn BlobStore>, uri: &str) {
    if let Some((bucket, key)) = parse_blob_uri(uri) {
        if let Err(err) = store.delete(&bucket, &key).await {
            match err {
                BlobError::NotFound(_) | BlobError::NoSuchBucket(_) => {}
                other => log::warn!("failed to delete artifact object {uri}: {other}"),
            }
        }
        return;
    }
    if let Err(err) = tokio::fs::remove_file(uri).await
        && err.kind() != std::io::ErrorKind::NotFound
    {
        log::warn!("failed to unlink artifact file {uri}: {err}");
    }
}

/// reduce a user-supplied filename to something safe to put in a key. keys are validated by the
/// store as well, so this is about keeping the key readable rather than about safety.
fn safe_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "artifact".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
#[path = "artifact_storage_tests.rs"]
mod tests;
