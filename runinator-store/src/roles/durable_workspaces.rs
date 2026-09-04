//! Portable workspace identities, snapshots, and fenced execution checkouts.

use runinator_models::{errors::SendableError, workspaces::*};
use std::future::Future;
use uuid::Uuid;

pub trait DurableWorkspaceStore: Send + Sync + 'static {
    fn create_durable_workspace(
        &self,
        workspace: DurableWorkspace,
        ownership: runinator_models::rbac::ResourceOwnership,
    ) -> impl Future<Output = Result<DurableWorkspace, SendableError>> + Send;
    fn resolve_durable_workspace(
        &self,
        org_id: Option<Uuid>,
        key: String,
    ) -> impl Future<Output = Result<Option<DurableWorkspace>, SendableError>> + Send;
    fn fetch_durable_workspace(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<DurableWorkspace>, SendableError>> + Send;
    fn list_durable_workspaces(
        &self,
        org_id: Option<Uuid>,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<Vec<DurableWorkspace>, SendableError>> + Send;
    fn fetch_workspace_snapshot(
        &self,
        id: Uuid,
        version: i64,
    ) -> impl Future<Output = Result<Option<WorkspaceSnapshot>, SendableError>> + Send;
    fn list_workspace_snapshots(
        &self,
        id: Uuid,
        limit: i64,
        offset: i64,
    ) -> impl Future<Output = Result<Vec<WorkspaceSnapshot>, SendableError>> + Send;
    fn acquire_workspace_checkout(
        &self,
        request: WorkspaceAcquire,
    ) -> impl Future<Output = Result<WorkspaceAcquisition, SendableError>> + Send;
    fn release_workspace_checkout(
        &self,
        id: Uuid,
        fence: i64,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn fetch_workspace_checkout(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkspaceCheckout>, SendableError>> + Send;
    fn delete_durable_workspace(
        &self,
        id: Uuid,
        version: Option<i64>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn prune_workspace_leases(&self) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn pending_workspace_cleanup(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkspaceSnapshot>, SendableError>> + Send;
    fn finish_workspace_cleanup(
        &self,
        id: Uuid,
        version: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn workspace_references_archive(
        &self,
        uri: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn workspace_version_for_run(
        &self,
        id: Uuid,
        run_id: Uuid,
    ) -> impl Future<Output = Result<Option<i64>, SendableError>> + Send;
}
