//! the durable record of execution: run/node-run claims, chunks, artifacts, orchestration events, and the ready-node queue.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_models::{
    errors::SendableError,
    workflows::{WorkflowRun, WorkflowStatus},
};

/// Core persistence operations for Runinator.
/// The durable record of execution: run/node-run claims, chunks, artifacts, orchestration events, and the ready-node queue.
pub trait RunStore: Send + Sync + 'static {
    /// Fetch workflow runs filtered by status.
    fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Claim open workflow runs for scheduler processing until the supplied lease instant.
    fn claim_workflow_runs_for_scheduler(
        &self,
        scheduler_id: String,
        statuses: Vec<WorkflowStatus>,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Renew a workflow run claim held by a scheduler.
    fn renew_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Release a workflow run claim held by a scheduler.
    fn release_workflow_run_claim(
        &self,
        workflow_run_id: Uuid,
        scheduler_id: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch the most recent workflow runs across all definitions, newest first, capped at `limit`.
    fn fetch_recent_workflow_runs(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Permanently delete a workflow run and all of its execution history.
    fn delete_workflow_run(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch runs that are still open (non-terminal) and were created before `cutoff`, for the
    /// duration-based notification scanner.
    fn fetch_open_workflow_runs_created_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;
}
