#[allow(unused_imports)]
use super::*;

pub(crate) struct ObjectGraphStorageProvider<T> {
    pub(super) context: WorkspaceStorageContext<T>,
    pub(crate) metadata_reads: std::sync::Arc<tokio::sync::Semaphore>,
    pub(crate) object_cache: std::sync::Arc<runinator_workspace::storage::cache::BufferedCache>,
    pub(crate) records: std::sync::Arc<runinator_workspace::storage::cache::ByteCache>,
    pub(crate) decoded_records: std::sync::Arc<runinator_workspace::storage::cache::ByteCache>,
}

impl<T> ObjectGraphStorageProvider<T> {
    pub(crate) fn new(context: WorkspaceStorageContext<T>) -> Self {
        Self {
            context,
            records: std::sync::Arc::new(runinator_workspace::storage::cache::ByteCache::new(
                32 * 1024 * 1024,
            )),
            decoded_records: std::sync::Arc::new(
                runinator_workspace::storage::cache::ByteCache::new(16 * 1024 * 1024),
            ),
            metadata_reads: std::sync::Arc::new(tokio::sync::Semaphore::new(8)),
            object_cache: std::sync::Arc::new(
                runinator_workspace::storage::cache::BufferedCache::new(32 * 1024 * 1024),
            ),
        }
    }
}

impl<T> std::ops::Deref for ObjectGraphStorageProvider<T> {
    type Target = WorkspaceStorageContext<T>;

    fn deref(&self) -> &Self::Target {
        &self.context
    }
}

impl<T: DurableWorkspaceStore> ObjectGraphStorageProvider<T> {
    pub(crate) async fn snapshot(
        &self,
        id: uuid::Uuid,
        version: i64,
    ) -> Result<WorkspaceSnapshot, SendableError> {
        self.store
            .fetch_workspace_snapshot(id, version)
            .await?
            .ok_or_else(|| runinator_models::errors::WORKSPACE_MISSING.error("version not found"))
    }

    pub(crate) async fn checkout(
        &self,
        id: uuid::Uuid,
    ) -> Result<WorkspaceCheckout, SendableError> {
        self.store
            .fetch_workspace_checkout(id)
            .await?
            .ok_or_else(|| {
                runinator_models::errors::WORKSPACE_CONFLICT.error("checkout is no longer active")
            })
    }
}

#[async_trait]
impl<T> WorkspaceStorageProvider<T> for ObjectGraphStorageProvider<T>
where
    T: DurableWorkspaceStore + runinator_store::roles::WorkflowVmStore,
{
    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn configure_limits(&self, limits: WorkspaceLimits) {
        if let Ok(mut configured) = self.limits.write() {
            *configured = limits;
        }
    }

    async fn checkout_content(
        &self,
        checkout: WorkspaceCheckout,
    ) -> Result<WorkspaceContent, SendableError> {
        ObjectGraphStorageProvider::checkout_content(self, checkout).await
    }

    async fn object(
        &self,
        workspace: uuid::Uuid,
        id: String,
        version: i64,
    ) -> Result<Vec<u8>, SendableError> {
        ObjectGraphStorageProvider::object(self, workspace, id, version).await
    }

    async fn stage_checkout(&self, id: uuid::Uuid, bytes: Vec<u8>) -> Result<(), SendableError> {
        ObjectGraphStorageProvider::upload_pack(self, id, bytes).await
    }

    async fn seal_checkout(
        &self,
        id: uuid::Uuid,
        request: WorkspaceSeal,
    ) -> Result<WorkspaceReceipt, SendableError> {
        ObjectGraphStorageProvider::seal(self, id, request).await
    }

    async fn directory(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        ObjectGraphStorageProvider::directory(self, id, version, path, after, limit).await
    }

    async fn results(
        &self,
        id: uuid::Uuid,
        version: i64,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError> {
        ObjectGraphStorageProvider::results(self, id, version, after, limit).await
    }

    async fn result_content(
        &self,
        id: uuid::Uuid,
        version: i64,
        name: String,
        preview: bool,
    ) -> Result<WorkspaceContent, SendableError> {
        ObjectGraphStorageProvider::result_content(self, id, version, name, preview).await
    }

    async fn file_range(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, SendableError> {
        ObjectGraphStorageProvider::file_range(self, id, version, path, offset, length).await
    }

    async fn file(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
    ) -> Result<WorkspaceContent, SendableError> {
        ObjectGraphStorageProvider::file(self, id, version, path).await
    }

    async fn diff(
        &self,
        id: uuid::Uuid,
        before: i64,
        after: i64,
        cursor: Option<String>,
    ) -> Result<WorkspaceDiff, SendableError> {
        ObjectGraphStorageProvider::diff(self, id, before, after, cursor).await
    }

    async fn process_transfer(
        &self,
        job: &WorkspaceTransfer,
        progress: &super::super::workspace_transfers::WorkspaceTransferProgress,
    ) -> Result<(), SendableError> {
        if job.importing {
            self.import_transfer(job, progress).await
        } else {
            self.export_transfer(job, progress).await
        }
    }

    async fn collect_workspaces(&self) -> Result<(), SendableError> {
        for id in self.store.workspace_gc_candidates().await? {
            ObjectGraphStorageProvider::collect_workspace(self, id).await?;
        }
        Ok(())
    }

    async fn cleanup_effect_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        ObjectGraphStorageProvider::cleanup_effect_orphans(self, cursor).await
    }

    async fn cleanup_native_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        ObjectGraphStorageProvider::cleanup_native_orphans(self, cursor).await
    }

    async fn cleanup_transfer_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        let page =
            crate::artifact_storage::workspace_transfer_upload_page(&self.blobs, cursor).await?;
        for object in page.objects {
            if object.last_modified > chrono::Utc::now() - chrono::Duration::hours(24) {
                continue;
            }
            let Some(path) = object.key.strip_prefix("transfers/") else {
                continue;
            };
            let Some((id, _)) = path.split_once('/') else {
                continue;
            };
            let Ok(id) = id.parse::<uuid::Uuid>() else {
                continue;
            };
            let key = runinator_blob_core::ObjectKey::parse(&object.key)?;
            let uri = runinator_blob_core::blob_uri(runinator_blob_core::WORKSPACE_BUCKET, &key);
            if let Some(job) = self.store.fetch_workspace_transfer(id).await?
                && job.expires_at > chrono::Utc::now()
                && (job.archive_uri.as_ref() == Some(&uri)
                    || ["receiving", "running"].contains(&job.state.as_str()))
            {
                continue;
            }
            crate::artifact_storage::delete_artifact_checked(&self.blobs, &uri).await?;
        }
        Ok(page.next_continuation_token)
    }

    async fn cleanup_deleted_snapshots(&self) -> Result<(), SendableError> {
        for snapshot in self.store.pending_workspace_cleanup().await? {
            // object graph bytes remain addressable until generation-fenced collection marks roots.
            self.store
                .finish_workspace_cleanup(snapshot.workspace_id, snapshot.version)
                .await?;
        }
        Ok(())
    }
}
