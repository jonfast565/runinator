//! Durable workspace management and content transfers.
pub use crate::artifact_storage::ArtifactContent as WorkspaceContent;
use crate::repository::durable_workspaces::validate_key;
use chrono::Utc;
use runinator_models::{
    errors::{SendableError, WORKSPACE_CONFLICT, WORKSPACE_INVALID},
    workspaces::*,
};
use runinator_store::roles::DurableWorkspaceStore;

/// Management and data-plane storage; handlers never reach the persistence store directly.
pub struct WorkspaceService<T> {
    pub(super) store: std::sync::Arc<T>,
    pub(super) limits: WorkspaceLimits,
    pub(super) blobs: std::sync::Arc<dyn runinator_blob_core::BlobStore>,
}

impl<T: DurableWorkspaceStore> WorkspaceService<T> {
    pub fn new(
        store: std::sync::Arc<T>,
        blobs: std::sync::Arc<dyn runinator_blob_core::BlobStore>,
    ) -> Self {
        Self {
            store,
            blobs,
            limits: WorkspaceLimits::default(),
        }
    }
    pub fn with_limits(mut self, limits: WorkspaceLimits) -> Self {
        self.limits = limits;
        self
    }
    pub async fn list(
        &self,
        org: Option<uuid::Uuid>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<DurableWorkspace>, SendableError> {
        self.store.list_durable_workspaces(org, limit, offset).await
    }
    pub async fn get(&self, id: uuid::Uuid) -> Result<Option<DurableWorkspace>, SendableError> {
        self.store.fetch_durable_workspace(id).await
    }
    pub async fn versions(
        &self,
        id: uuid::Uuid,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<WorkspaceSnapshot>, SendableError> {
        self.store.list_workspace_snapshots(id, limit, offset).await
    }
    pub async fn snapshot(
        &self,
        id: uuid::Uuid,
        version: i64,
    ) -> Result<WorkspaceSnapshot, SendableError> {
        self.store
            .fetch_workspace_snapshot(id, version)
            .await?
            .ok_or_else(|| runinator_models::errors::WORKSPACE_MISSING.error("version not found"))
    }
    pub async fn create(
        &self,
        workspace: DurableWorkspace,
        ownership: runinator_models::rbac::ResourceOwnership,
    ) -> Result<DurableWorkspace, SendableError> {
        validate_key(&workspace.key)?;
        self.store
            .create_durable_workspace(workspace, ownership)
            .await
    }
    pub async fn checkout(&self, id: uuid::Uuid) -> Result<WorkspaceCheckout, SendableError> {
        self.store
            .fetch_workspace_checkout(id)
            .await?
            .ok_or_else(|| WORKSPACE_CONFLICT.error("checkout is no longer active"))
    }
    pub async fn delete(
        &self,
        id: uuid::Uuid,
        version: Option<i64>,
    ) -> Result<bool, SendableError> {
        let deleted = self.store.delete_durable_workspace(id, version).await?;
        if let Err(error) = self.cleanup().await {
            tracing::warn!(%error, "workspace byte deletion will retry");
        }
        Ok(deleted)
    }
    pub async fn cleanup(&self) -> Result<(), SendableError> {
        self.store.prune_workspace_leases().await?;
        for snapshot in self.store.pending_workspace_cleanup().await? {
            // objects remain addressable until generation-fenced collection has marked retained roots.
            self.store
                .finish_workspace_cleanup(snapshot.workspace_id, snapshot.version)
                .await?;
        }
        Ok(())
    }
}

impl<
    T: DurableWorkspaceStore
        + runinator_store::roles::WorkflowVmStore
        + runinator_store::roles::ReplicaStore,
> WorkspaceService<T>
{
    pub async fn require_assigned_checkout(
        &self,
        id: uuid::Uuid,
        replica_id: uuid::Uuid,
        ctx: &runinator_models::auth::AuthContext,
    ) -> Result<WorkspaceCheckout, SendableError> {
        let checkout = self.checkout(id).await?;
        let effect = self
            .store
            .fetch_workflow_effect(checkout.effect_id)
            .await?
            .ok_or_else(|| WORKSPACE_INVALID.error("assigned effect is missing"))?;
        if effect.attempt != checkout.attempt
            || effect.status.is_terminal()
            || effect.current_executor_replica_id != Some(replica_id)
        {
            return Err(WORKSPACE_CONFLICT.error("replica has not claimed this active attempt"));
        }
        let replica = self
            .store
            .fetch_replica(replica_id)
            .await?
            .ok_or_else(|| WORKSPACE_INVALID.error("assigned replica is missing"))?;
        if !ctx.is_platform_admin() {
            let authorized = match replica.registered_by_principal_id {
                Some(owner) => ctx.principal_id == Some(owner),
                None => ctx.system_role == Some(runinator_models::rbac::SystemRole::Worker),
            };
            if !authorized {
                return Err(WORKSPACE_INVALID.error("principal does not own the assigned replica"));
            }
        }
        Ok(checkout)
    }
}

pub async fn run_workspace_storage_cleanup<
    T: DurableWorkspaceStore + runinator_store::roles::WorkflowVmStore,
>(
    service: std::sync::Arc<WorkspaceService<T>>,
    shutdown: std::sync::Arc<tokio::sync::Notify>,
) {
    let mut cursor = None;
    let mut native_cursor = None;
    let mut transfer_cursor = None;
    loop {
        match service
            .cleanup_transfer_archives(transfer_cursor.take())
            .await
        {
            Ok(next) => transfer_cursor = next,
            Err(error) => tracing::warn!(%error, "workspace transfer cleanup will retry"),
        }
        match service.cleanup_orphans(cursor.take()).await {
            Ok(next) => cursor = next,
            Err(error) => tracing::warn!(%error, "workspace orphan cleanup will retry"),
        }
        match service.cleanup_native_orphans(native_cursor.take()).await {
            Ok(next) => native_cursor = next,
            Err(error) => tracing::warn!(%error, "native workspace orphan cleanup will retry"),
        }
        if let Err(error) = service.collect_workspaces().await {
            tracing::warn!(%error, "workspace collection will retry");
        }
        if let Err(error) = service.cleanup().await {
            tracing::warn!(%error, "workspace storage cleanup will retry");
        }
        tokio::select! {
            _ = shutdown.notified() => return,
            _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => {},
        }
    }
}

impl<T: DurableWorkspaceStore + runinator_store::roles::WorkflowVmStore> WorkspaceService<T> {
    async fn cleanup_orphans(
        &self,
        cursor: Option<String>,
    ) -> Result<Option<String>, SendableError> {
        let page = crate::artifact_storage::workspace_upload_page(&self.blobs, cursor).await?;
        let cutoff = Utc::now() - chrono::Duration::hours(24);
        for object in page.objects {
            if object.last_modified > cutoff {
                continue;
            }
            let Some(effect_id) = object
                .key
                .strip_prefix("effects/")
                .and_then(|key| key.split('/').next())
                .and_then(|id| uuid::Uuid::parse_str(id).ok())
            else {
                continue;
            };
            // terminal effects cannot be retried in place; check this before reference lookup so
            // a concurrent successful settlement cannot make a committed upload look orphaned.
            if self
                .store
                .fetch_workflow_effect(effect_id)
                .await?
                .is_some_and(|effect| !effect.status.is_terminal())
            {
                continue;
            }
            let uri = runinator_blob_core::blob_uri(
                runinator_blob_core::WORKSPACE_BUCKET,
                &runinator_blob_core::ObjectKey::parse(&object.key)?,
            );
            if !self.store.workspace_references_archive(uri.clone()).await? {
                crate::artifact_storage::delete_artifact_checked(&self.blobs, &uri).await?;
            }
        }
        Ok(page.next_continuation_token)
    }
}
