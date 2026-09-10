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
    pub sha256: Option<String>,
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
        sha256: (!reader.meta.sha256.is_empty()).then_some(reader.meta.sha256),
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

pub fn workspace_pack_uri(workspace: Uuid, pack: &str) -> Result<String, SendableError> {
    let (scope, digest) = pack
        .split_once('/')
        .ok_or_else(|| runinator_models::errors::WORKSPACE_INVALID.error("invalid pack key"))?;
    if Uuid::parse_str(scope).is_err()
        || digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(runinator_models::errors::WORKSPACE_INVALID.error("invalid pack key"));
    }
    Ok(blob_uri(
        runinator_blob_core::WORKSPACE_BUCKET,
        &ObjectKey::parse(&format!("native/{workspace}/{pack}.pack"))?,
    ))
}

pub async fn put_workspace_pack(
    store: &Arc<dyn BlobStore>,
    workspace: Uuid,
    scope: Uuid,
    bytes: Vec<u8>,
) -> Result<String, SendableError> {
    let digest = format!("{scope}/{}", runinator_blob_core::sha256_hex(&bytes));
    let uri = workspace_pack_uri(workspace, &digest)?;
    let (_, key) =
        parse_blob_uri(&uri).ok_or_else(|| ARTIFACT_STORE_FAILED.error("invalid pack URI"))?;
    store
        .put(
            runinator_blob_core::WORKSPACE_BUCKET,
            &key,
            bytes,
            PutOptions {
                content_type: Some("application/vnd.runinator.workspace.pack.v1".into()),
                ..Default::default()
            },
        )
        .await?;
    Ok(digest)
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

pub async fn workspace_native_upload_page(
    store: &Arc<dyn BlobStore>,
    cursor: Option<String>,
) -> Result<runinator_blob_core::ListResponse, SendableError> {
    Ok(store
        .list(
            runinator_blob_core::WORKSPACE_BUCKET,
            &runinator_blob_core::ListRequest {
                prefix: Some("native/".into()),
                continuation_token: cursor,
                max_keys: Some(1000),
                ..Default::default()
            },
        )
        .await?)
}

pub async fn workspace_transfer_upload_page(
    store: &Arc<dyn BlobStore>,
    cursor: Option<String>,
) -> Result<runinator_blob_core::ListResponse, SendableError> {
    Ok(store
        .list(
            runinator_blob_core::WORKSPACE_BUCKET,
            &runinator_blob_core::ListRequest {
                prefix: Some("transfers/".into()),
                continuation_token: cursor,
                max_keys: Some(1000),
                ..Default::default()
            },
        )
        .await?)
}

/// Stream a transfer archive to shared storage with bounded multipart buffers.
pub async fn put_workspace_transfer<R: AsyncRead + Send + Unpin>(
    store: &Arc<dyn BlobStore>,
    id: Uuid,
    token: Uuid,
    mut reader: R,
    limit: u64,
    idle_timeout: Option<std::time::Duration>,
) -> Result<String, SendableError> {
    use runinator_blob_core::{CompletedPart, WORKSPACE_BUCKET};
    use tokio::io::AsyncReadExt;
    let key = ObjectKey::parse(&format!("transfers/{id}/{token}.oci.tar"))?;
    let upload = store
        .create_multipart(WORKSPACE_BUCKET, &key, PutOptions::default())
        .await?;
    let result = async {
        let mut total = 0u64;
        let mut parts = Vec::new();
        loop {
            let mut bytes = vec![0; 64 * 1024 * 1024];
            let mut filled = 0;
            while filled < bytes.len() {
                let n = if let Some(timeout) = idle_timeout {
                    tokio::time::timeout(timeout, reader.read(&mut bytes[filled..])).await??
                } else {
                    reader.read(&mut bytes[filled..]).await?
                };
                if n == 0 {
                    break;
                }
                filled += n;
                total = total.checked_add(n as u64).ok_or_else(|| {
                    runinator_models::errors::WORKSPACE_LIMIT.error("transfer size overflow")
                })?;
                if total > limit {
                    return Err(runinator_models::errors::WORKSPACE_LIMIT
                        .error("archive exceeds transfer budget"));
                }
            }
            if filled == 0 && !parts.is_empty() {
                break;
            }
            bytes.truncate(filled);
            let number = u32::try_from(parts.len() + 1)?;
            if number > runinator_blob_core::MAX_PART_NUMBER {
                return Err(runinator_models::errors::WORKSPACE_LIMIT
                    .error("archive exceeds object-store multipart capacity"));
            }
            let etag = store
                .upload_part(WORKSPACE_BUCKET, &key, &upload, number, bytes)
                .await?;
            parts.push(CompletedPart {
                part_number: number,
                etag,
            });
            if filled < 64 * 1024 * 1024 {
                break;
            }
        }
        store
            .complete_multipart(WORKSPACE_BUCKET, &key, &upload, &parts)
            .await?;
        Ok::<_, SendableError>(blob_uri(WORKSPACE_BUCKET, &key))
    }
    .await;
    if result.is_err() {
        let _ = store.abort_multipart(WORKSPACE_BUCKET, &key, &upload).await;
    }
    result
}
