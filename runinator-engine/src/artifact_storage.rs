//! where artifact bytes live.
//!
//! Artifact URIs are always `blob://` URIs, so bytes are available to every replica.

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

/// open an artifact's bytes for streaming from the object store.
pub async fn open_artifact(
    store: &Arc<dyn BlobStore>,
    uri: &str,
    range: Option<ByteRange>,
) -> Result<ArtifactContent, SendableError> {
    let (bucket, key) = parse_blob_uri(uri)
        .ok_or_else(|| ARTIFACT_UNREADABLE.error(format!("invalid artifact URI {uri}")))?;
    let reader = store
        .open(&bucket, &key, range)
        .await
        .map_err(|err| ARTIFACT_UNREADABLE.error(err))?;
    Ok(ArtifactContent {
        size_bytes: reader.len(),
        body: reader.body,
    })
}

/// remove an artifact's bytes. a missing object must not block deleting the row that points at it,
/// or the row becomes undeletable.
pub async fn delete_artifact_bytes(store: &Arc<dyn BlobStore>, uri: &str) {
    let Some((bucket, key)) = parse_blob_uri(uri) else {
        log::warn!("cannot delete artifact with invalid blob URI {uri}");
        return;
    };
    if let Err(err) = store.delete(&bucket, &key).await {
        match err {
            BlobError::NotFound(_) | BlobError::NoSuchBucket(_) => {}
            other => log::warn!("failed to delete artifact object {uri}: {other}"),
        }
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

/// Delete bytes while retaining failures for durable cleanup retries.
pub async fn delete_artifact_checked(
    store: &Arc<dyn BlobStore>,
    uri: &str,
) -> Result<(), SendableError> {
    let (bucket, key) =
        parse_blob_uri(uri).ok_or_else(|| ARTIFACT_UNREADABLE.error("invalid artifact URI"))?;
    match store.delete(&bucket, &key).await {
        Ok(()) | Err(BlobError::NotFound(_)) | Err(BlobError::NoSuchBucket(_)) => Ok(()),
        Err(error) => Err(ARTIFACT_STORE_FAILED.error(error)),
    }
}

/// Snapshot uploads are content-addressed within their producing effect.
pub async fn put_workspace_snapshot(
    store: &Arc<dyn BlobStore>,
    effect_id: Uuid,
    bytes: Vec<u8>,
) -> Result<String, SendableError> {
    let digest = runinator_blob_core::sha256_hex(&bytes);
    let key = ObjectKey::parse(&format!("effects/{effect_id}/{digest}.tar.gz"))
        .map_err(|error| ARTIFACT_STORE_FAILED.error(error))?;
    store
        .put(
            runinator_blob_core::WORKSPACE_BUCKET,
            &key,
            bytes,
            PutOptions {
                content_type: Some("application/gzip".into()),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| ARTIFACT_STORE_FAILED.error(error))?;
    Ok(blob_uri(runinator_blob_core::WORKSPACE_BUCKET, &key))
}

pub async fn workspace_upload_page(
    store: &Arc<dyn BlobStore>,
    cursor: Option<String>,
) -> Result<runinator_blob_core::ListResponse, SendableError> {
    store
        .list(
            runinator_blob_core::WORKSPACE_BUCKET,
            &runinator_blob_core::ListRequest {
                prefix: Some("effects/".into()),
                continuation_token: cursor,
                max_keys: Some(1000),
                ..Default::default()
            },
        )
        .await
        .map_err(|error| ARTIFACT_STORE_FAILED.error(error))
}
