//! the durable record of execution: run/node-run claims, chunks, artifacts, orchestration events, and the ready-node queue.
//!
//! one of the role traits `DatabaseImpl` composes. bound on this directly when a caller only
//! needs this slice of the store.

use std::future::Future;

use super::QueueSnapshot;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use runinator_comm::WorkflowResultEvent;
use runinator_models::{
    errors::SendableError,
    orchestration::{
        NewOrchestrationEvent, NodeTransition, NodeTransitionStat, OrchestrationEvent,
        ReadyNodeRecord,
    },
    runs::{NewRunArtifact, NewRunChunk},
    workflows::{
        WorkflowNodeRun, WorkflowNodeRunArtifact, WorkflowNodeRunChunk, WorkflowRun,
        WorkflowRunArtifact, WorkflowStatus,
    },
};

// re-exported here so callers that reach for the contract at its historical path
// (`runinator_database::interfaces::*`) can import both halves from one place.
pub use crate::reducer_store::ReducerStore;

/// Core persistence operations for Runinator.
/// The durable record of execution: run/node-run claims, chunks, artifacts, orchestration events, and the ready-node queue.
pub trait RunStore: Send + Sync + 'static {
    /// Operational snapshots of due and future ready work, respectively.
    fn ready_node_queue_snapshots(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<(QueueSnapshot, QueueSnapshot), SendableError>> + Send;

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

    /// Acquire the executor lease for a node run, returning whether it was acquired. The claim only
    /// succeeds when no live executor holds the slot, making duplicate/redelivered executions of the
    /// same node run mutually exclusive. A slot is free when unclaimed, when the prior claim predates
    /// `stale_before` (the action's own deadline), or when the holding replica is no longer live —
    /// offline, or last heartbeating before `heartbeat_stale_before`. The heartbeat arm is what keeps
    /// a crashed worker from stranding the node for its full timeout window.
    fn claim_workflow_node_run_executor(
        &self,
        node_run_id: Uuid,
        replica_id: Uuid,
        claimed_at: DateTime<Utc>,
        stale_before: DateTime<Utc>,
        heartbeat_stale_before: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch a node execution record by its identifier.
    fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: Uuid,
    ) -> impl Future<Output = Result<Option<WorkflowNodeRun>, SendableError>> + Send;

    /// Append a log chunk to a workflow node run.
    fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: Uuid,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<WorkflowNodeRunChunk, SendableError>> + Send;

    /// Fetch log chunks for a workflow node run with pagination.
    fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: Uuid,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunChunk>, SendableError>> + Send;

    /// Associate an artifact with a workflow node run.
    fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: Uuid,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<WorkflowNodeRunArtifact, SendableError>> + Send;

    /// Fetch artifacts for a workflow node run.
    fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunArtifact>, SendableError>> + Send;

    /// Fetch run-level artifacts declared by output nodes for a workflow run.
    fn fetch_workflow_run_artifacts(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<WorkflowRunArtifact>, SendableError>> + Send;

    /// Apply a workflow result event once; returns false for duplicate events.
    fn apply_workflow_result_event(
        &self,
        event: &WorkflowResultEvent,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Append an internal orchestration event once; returns false for duplicate event ids.
    fn append_orchestration_event(
        &self,
        event: &NewOrchestrationEvent,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch internal orchestration events for a workflow run.
    fn fetch_orchestration_events(
        &self,
        workflow_run_id: Uuid,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationEvent>, SendableError>> + Send;

    /// Reconstruct the ordered edges a workflow run walked from its node-run chain.
    fn fetch_run_transitions(
        &self,
        workflow_run_id: Uuid,
    ) -> impl Future<Output = Result<Vec<NodeTransition>, SendableError>> + Send;

    /// Aggregate `from_node -> to_node` edges across all runs of a workflow. When `node_id` is
    /// set, only edges leaving that node are returned.
    fn fetch_node_transition_stats(
        &self,
        workflow_id: Uuid,
        node_id: Option<String>,
    ) -> impl Future<Output = Result<Vec<NodeTransitionStat>, SendableError>> + Send;

    /// Claim ready nodes for scheduler processing until the supplied lease instant.
    fn claim_ready_nodes(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReadyNodeRecord>, SendableError>> + Send;

    /// Fetch a ready-node row by identifier.
    fn fetch_ready_node(
        &self,
        ready_node_id: Uuid,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Mark a claimed ready-node row complete.
    fn complete_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch ready-node rows still pending drive (uncompleted and not currently claimed), so the
    /// web service can announce them on the wake channel. Includes future `ready_at` rows.
    fn fetch_pending_ready_nodes(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReadyNodeRecord>, SendableError>> + Send;

    /// Claim pending ready-node rows for wake announcement, stamping an announce lease of
    /// `lease_seconds` past the later of `now` and each row's `ready_at`. A row is returned at most
    /// once per lease window, so broker backends without in-flight dedupe do not accumulate
    /// duplicate wakes; the lease expiring re-announces a wake that was lost in flight.
    fn claim_ready_nodes_for_announce(
        &self,
        now: DateTime<Utc>,
        lease_seconds: i64,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReadyNodeRecord>, SendableError>> + Send;

    /// Claim a single ready-node row by id for drive, leasing it to `scheduler_id`.
    fn claim_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Release a claimed ready-node row back to the queued state so it can be re-driven.
    fn release_ready_node(
        &self,
        ready_node_id: Uuid,
        scheduler_id: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Safety backstop: settle up to `limit` uncompleted ready-node rows whose workflow run is
    /// already terminal. The reducer settles these inline on the terminal transition; this catches
    /// rows orphaned when that path did not run to completion — a crash mid-transition, or work
    /// enqueued before the inline cleanup existed — so the wake publisher stops rescanning dead runs.
    /// Returns the number of rows settled.
    fn settle_terminal_run_ready_nodes(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;

    /// Fetch runs that are still open (non-terminal) and were created before `cutoff`, for the
    /// duration-based notification scanner.
    fn fetch_open_workflow_runs_created_before(
        &self,
        cutoff: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;
}
