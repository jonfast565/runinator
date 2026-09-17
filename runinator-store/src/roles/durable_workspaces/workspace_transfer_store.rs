#[allow(unused_imports)]
use super::*;

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
