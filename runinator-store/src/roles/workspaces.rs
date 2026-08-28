//! Durable ownership and CAS transitions for admission-scoped worker-local workspaces.

use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    errors::SendableError,
    value::Value,
    workspaces::{NewWorkspaceLease, WorkspaceLease, WorkspaceStatus},
};
use uuid::Uuid;

pub trait WorkspaceStore: Send + Sync + 'static {
    /// Idempotently allocate the workspace identified by `(admission, generation, scope, attempt)`.
    fn allocate_workspace(
        &self,
        workspace: NewWorkspaceLease,
    ) -> impl Future<Output = Result<WorkspaceLease, SendableError>> + Send;

    fn fetch_workspace(
        &self,
        workspace_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkspaceLease>, SendableError>> + Send;

    fn fetch_workspace_attempt(
        &self,
        admission_id: Uuid,
        generation: i64,
        scope: String,
        attempt: i64,
    ) -> impl Future<Output = Result<Option<WorkspaceLease>, SendableError>> + Send;

    fn fetch_workspaces_for_admission(
        &self,
        admission_id: Uuid,
        generation: i64,
    ) -> impl Future<Output = Result<Vec<WorkspaceLease>, SendableError>> + Send;

    /// Advance a workspace only while its current CAS version and state match.
    fn transition_workspace_cas(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        expected_status: WorkspaceStatus,
        next_status: WorkspaceStatus,
        evidence: Option<Value>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn renew_workspace(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        worker_instance_id: String,
        worker_replica_id: Option<Uuid>,
        leased_until: DateTime<Utc>,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    fn mark_workspace_unavailable(
        &self,
        worker_instance_id: String,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    fn fetch_expired_workspaces(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkspaceLease>, SendableError>> + Send;

    fn fetch_finalizing_workspaces(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkspaceLease>, SendableError>> + Send;

    /// Re-scan abandoned rows so admission notification remains durable across a crash between
    /// the workspace CAS and inbox insertion. Inbox delivery IDs keep the replay idempotent.
    fn fetch_abandoned_workspaces(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkspaceLease>, SendableError>> + Send;

    fn mark_workspace_abandonment_notified(
        &self,
        workspace_id: Uuid,
        expected_version: i64,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;
}
