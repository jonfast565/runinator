//! Shared blob reads adapted to the synchronous storage algorithms on blocking workers.
use runinator_models::{errors::SendableError, workspaces::*};
use runinator_store::roles::DurableWorkspaceStore;
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::Arc;

async fn read_pack_range_from(
    blobs: Arc<dyn runinator_blob_core::BlobStore>,
    workspace: uuid::Uuid,
    pack: &str,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>, SendableError> {
    use tokio::io::AsyncReadExt;
    let uri = crate::artifact_storage::workspace_pack_uri(workspace, pack)?;
    let end = offset
        .checked_add(length)
        .and_then(|n| n.checked_sub(1))
        .ok_or_else(|| storage::Error::Corrupt("invalid object range".into()))?;
    let content = crate::artifact_storage::open_artifact(
        &blobs,
        &uri,
        Some(runinator_blob_core::ByteRange::From {
            start: offset,
            end: Some(end),
        }),
    )
    .await?;
    let mut bytes = Vec::new();
    content
        .body
        .take(length + 1)
        .read_to_end(&mut bytes)
        .await?;
    if bytes.len() as u64 != length {
        return Err(storage::Error::Corrupt("workspace object range is truncated".into()).into());
    }
    Ok(bytes)
}

pub(super) fn storage_error(error: SendableError) -> storage::Error {
    match error.downcast::<storage::Error>() {
        Ok(error) => *error,
        Err(error) => storage::Error::Io(std::io::Error::other(error)),
    }
}

mod shared_objects;
pub use shared_objects::SharedObjects;

mod lazy_reader_guard;
pub use lazy_reader_guard::LazyReaderGuard;

mod reader_guard;
pub use reader_guard::ReaderGuard;
