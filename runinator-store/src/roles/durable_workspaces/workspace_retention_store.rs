#[allow(unused_imports)]
use super::*;

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
