//! Portable workspace identities, snapshots, and fenced execution checkouts.

use runinator_models::{errors::SendableError, workspaces::*};
use std::future::Future;
use uuid::Uuid;

pub trait DurableWorkspaceStore:
    WorkspaceRetentionStore + WorkspaceTransferStore + Send + Sync + 'static
{
    fn create_workspace_download(
        &self,
        download: WorkspaceDownload,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn fetch_workspace_download(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkspaceDownload>, SendableError>> + Send;
    fn stage_workspace_objects(
        &self,
        checkout: WorkspaceCheckout,
        objects: Vec<WorkspaceObjectLocation>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn fetch_workspace_object(
        &self,
        workspace_id: Uuid,
        id: String,
    ) -> impl Future<Output = Result<Option<WorkspaceObjectLocation>, SendableError>> + Send;
    /// Page registered objects in one immutable pack, ordered by logical object identity.
    fn workspace_pack_objects(
        &self,
        workspace_id: Uuid,
        pack: String,
        after: Option<String>,
    ) -> impl Future<Output = Result<Vec<WorkspaceObjectLocation>, SendableError>> + Send;
    fn save_workspace_receipt(
        &self,
        receipt: WorkspaceReceipt,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
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

pub trait WorkspaceRetentionStore: Send + Sync + 'static {
    fn workspace_pack_needed(
        &self,
        workspace: Uuid,
        scope: Uuid,
        pack: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn workspace_gc_candidates(
        &self,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;
    fn claim_workspace_gc(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkspaceGcLease>, SendableError>> + Send;
    fn renew_workspace_gc(
        &self,
        lease: WorkspaceGcLease,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn stage_workspace_gc(
        &self,
        lease: WorkspaceGcLease,
        objects: Vec<WorkspaceObjectLocation>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn finish_workspace_gc(
        &self,
        lease: WorkspaceGcLease,
        repacked: bool,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn expired_workspace_packs(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Vec<String>, SendableError>> + Send;
    fn finish_workspace_pack_cleanup(
        &self,
        id: Uuid,
        pack: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn pin_workspace_reader(
        &self,
        id: Uuid,
        version: i64,
    ) -> impl Future<Output = Result<WorkspaceReaderLease, SendableError>> + Send;
    fn renew_workspace_reader(
        &self,
        lease: WorkspaceReaderLease,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn release_workspace_reader(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}

pub trait WorkspaceTransferStore: Send + Sync + 'static {
    fn create_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn fetch_workspace_transfer(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkspaceTransfer>, SendableError>> + Send;
    fn workspace_transfer_candidates(
        &self,
    ) -> impl Future<Output = Result<Vec<Uuid>, SendableError>> + Send;
    fn claim_workspace_transfer(
        &self,
        id: Uuid,
        uploading: bool,
    ) -> impl Future<Output = Result<Option<WorkspaceTransfer>, SendableError>> + Send;
    fn progress_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
        bytes: u64,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn finish_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
        state: String,
        archive: Option<String>,
        snapshot: Option<WorkspaceSnapshot>,
        error: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn cancel_workspace_transfer(
        &self,
        id: Uuid,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
    fn stage_workspace_transfer(
        &self,
        job: WorkspaceTransfer,
        objects: Vec<WorkspaceObjectLocation>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
}
