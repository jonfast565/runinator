#[allow(unused_imports)]
use super::*;

/// Representation-specific durable workspace operations.
///
/// Identity, authorization, transfer scheduling, and durable job claims remain in
/// [`WorkspaceService`]. Implementations own byte layout, validation, reads, transfers, and
/// retention for their representation.
#[async_trait]
pub(crate) trait WorkspaceStorageProvider<T>: Send + Sync
where
    T: DurableWorkspaceStore,
{
    #[cfg(test)]
    fn as_any(&self) -> &dyn std::any::Any;
    fn configure_limits(&self, limits: WorkspaceLimits);

    async fn checkout_content(
        &self,
        checkout: WorkspaceCheckout,
    ) -> Result<WorkspaceContent, SendableError>;
    async fn object(
        &self,
        workspace: uuid::Uuid,
        id: String,
        version: i64,
    ) -> Result<Vec<u8>, SendableError>;
    async fn stage_checkout(&self, id: uuid::Uuid, bytes: Vec<u8>) -> Result<(), SendableError>;
    async fn seal_checkout(
        &self,
        id: uuid::Uuid,
        request: WorkspaceSeal,
    ) -> Result<WorkspaceReceipt, SendableError>;
    async fn directory(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError>;
    async fn results(
        &self,
        id: uuid::Uuid,
        version: i64,
        after: Option<String>,
        limit: usize,
    ) -> Result<WorkspaceDirectory, SendableError>;
    async fn result_content(
        &self,
        id: uuid::Uuid,
        version: i64,
        name: String,
        preview: bool,
    ) -> Result<WorkspaceContent, SendableError>;
    async fn file_range(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
        offset: u64,
        length: usize,
    ) -> Result<Vec<u8>, SendableError>;
    async fn file(
        &self,
        id: uuid::Uuid,
        version: i64,
        path: String,
    ) -> Result<WorkspaceContent, SendableError>;
    async fn diff(
        &self,
        id: uuid::Uuid,
        before: i64,
        after: i64,
        cursor: Option<String>,
    ) -> Result<WorkspaceDiff, SendableError>;
    async fn process_transfer(
        &self,
        job: &WorkspaceTransfer,
        progress: &super::super::workspace_transfers::WorkspaceTransferProgress,
    ) -> Result<(), SendableError>;
    async fn collect_workspaces(&self) -> Result<(), SendableError>;
    async fn cleanup_effect_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError>;
    async fn cleanup_native_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError>;
    async fn cleanup_transfer_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError>;
    async fn cleanup_deleted_snapshots(&self) -> Result<(), SendableError>;
}
