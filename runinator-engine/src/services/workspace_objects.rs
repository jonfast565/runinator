//! Shared blob reads adapted to the synchronous storage algorithms on blocking workers.
use runinator_models::{errors::SendableError, workspaces::*};
use runinator_store::roles::DurableWorkspaceStore;
use runinator_workspace::storage::{
    self, Id,
    store::{Object, ObjectInfo, ReadStore},
};
use std::sync::Arc;

pub struct SharedObjects<T: DurableWorkspaceStore> {
    pub db: Arc<T>,
    pub blobs: Arc<dyn runinator_blob_core::BlobStore>,
    pub workspace: uuid::Uuid,
    pub runtime: tokio::runtime::Handle,
    pub reader: Option<ReaderGuard<T>>,
    pub records: storage::cache::ByteCache,
}

impl<T: DurableWorkspaceStore> SharedObjects<T> {
    async fn location(&self, id: Id) -> Result<WorkspaceObjectLocation, SendableError> {
        self.db
            .fetch_workspace_object(self.workspace, id.to_string())
            .await?
            .ok_or_else(|| Box::new(storage::Error::NotFound(id.to_string())) as SendableError)
    }
    fn read(&self, id: Id) -> storage::Result<Object> {
        if self.reader.as_ref().is_some_and(|reader| !reader.alive()) {
            return Err(storage::Error::Conflict);
        }
        let location = self
            .runtime
            .block_on(self.location(id))
            .map_err(storage_error)?;
        let key = Id::sha256(
            format!("{}:{}:{}", location.pack, location.offset, location.length).as_bytes(),
        );
        if location.length > storage::codec::MAX_OBJECT as u64 + storage::record::HEADER_LEN + 65536
        {
            return Err(storage::Error::Corrupt("oversized physical record".into()));
        }
        let bytes = self
            .records
            .get_or_load(key, location.length as usize, || {
                self.runtime
                    .block_on(self.read_record(&location))
                    .map_err(storage_error)
            })?;
        storage::record::decode_range(&bytes, id, location.member)
    }
    async fn read_record(
        &self,
        location: &WorkspaceObjectLocation,
    ) -> Result<Vec<u8>, SendableError> {
        use tokio::io::AsyncReadExt;
        let uri = crate::artifact_storage::workspace_pack_uri(self.workspace, &location.pack)?;
        let end = location
            .offset
            .checked_add(location.length)
            .and_then(|n| n.checked_sub(1))
            .ok_or_else(|| storage::Error::Corrupt("invalid object range".into()))?;
        let content = crate::artifact_storage::open_artifact(
            &self.blobs,
            &uri,
            Some(runinator_blob_core::ByteRange::From {
                start: location.offset,
                end: Some(end),
            }),
        )
        .await?;
        let mut bytes = Vec::new();
        content
            .body
            .take(location.length + 1)
            .read_to_end(&mut bytes)
            .await?;
        Ok(bytes)
    }
}

fn storage_error(error: SendableError) -> storage::Error {
    match error.downcast::<storage::Error>() {
        Ok(error) => *error,
        Err(error) => storage::Error::Io(std::io::Error::other(error)),
    }
}

impl<T: DurableWorkspaceStore> ReadStore for SharedObjects<T> {
    fn info(&self, id: Id) -> storage::Result<ObjectInfo> {
        let location = self
            .runtime
            .block_on(self.location(id))
            .map_err(storage_error)?;
        if location.raw_len > storage::codec::MAX_OBJECT as u64 {
            return Err(storage::Error::Corrupt(
                "invalid logical object length".into(),
            ));
        }
        Ok(ObjectInfo {
            kind: location.kind.try_into()?,
            raw_len: location.raw_len as usize,
        })
    }
    fn get(&self, id: Id) -> storage::Result<Object> {
        self.read(id)
    }
    fn contains(&self, id: Id) -> storage::Result<bool> {
        self.runtime
            .block_on(
                self.db
                    .fetch_workspace_object(self.workspace, id.to_string()),
            )
            .map(|object| object.is_some())
            .map_err(storage_error)
    }
}

pub struct ReaderGuard<T: DurableWorkspaceStore> {
    db: Arc<T>,
    lease: WorkspaceReaderLease,
    task: tokio::task::JoinHandle<()>,
    runtime: tokio::runtime::Handle,
    expires: Arc<std::sync::atomic::AtomicI64>,
}
impl<T: DurableWorkspaceStore> ReaderGuard<T> {
    pub async fn new(
        db: Arc<T>,
        workspace: uuid::Uuid,
        version: i64,
    ) -> Result<Self, SendableError> {
        let lease = db.pin_workspace_reader(workspace, version).await?;
        let expires = Arc::new(std::sync::atomic::AtomicI64::new(
            lease.expires_at.timestamp(),
        ));
        let renewing_db = db.clone();
        let renewing_lease = lease.clone();
        let renewing_expires = expires.clone();
        let task = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(60)).await;
                match renewing_db
                    .renew_workspace_reader(renewing_lease.clone())
                    .await
                {
                    Ok(true) => {
                        renewing_expires.store(
                            (chrono::Utc::now() + chrono::Duration::minutes(4)).timestamp(),
                            std::sync::atomic::Ordering::Release,
                        );
                    }
                    _ => {
                        renewing_expires.store(0, std::sync::atomic::Ordering::Release);
                        return;
                    }
                }
            }
        });
        Ok(Self {
            db,
            lease,
            task,
            runtime: tokio::runtime::Handle::current(),
            expires,
        })
    }
    fn alive(&self) -> bool {
        self.expires.load(std::sync::atomic::Ordering::Acquire) > chrono::Utc::now().timestamp()
    }
}
impl<T: DurableWorkspaceStore> Drop for ReaderGuard<T> {
    fn drop(&mut self) {
        self.task.abort();
        let db = self.db.clone();
        let id = self.lease.id;
        self.runtime.spawn(async move {
            let _ = db.release_workspace_reader(id).await;
        });
    }
}
