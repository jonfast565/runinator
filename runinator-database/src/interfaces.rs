use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_comm::{ActionCommand, ActionDispatchRecord, WorkflowResultEvent};
use runinator_models::value::Value;
use runinator_models::{
    errors::SendableError,
    notifications::{NewNotification, Notification},
    orchestration::{NewOrchestrationEvent, OrchestrationEvent, ReadyNodeRecord},
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{
        WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact, WorkflowNodeRunChunk,
        WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};

/// Core persistence operations for Runinator.
pub trait DatabaseImpl: Send + Sync + 'static {
    /// Execute initialization scripts for the database.
    fn run_init_scripts(
        &self,
        paths: &[String],
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch all runs filtered by their current status.
    fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> impl Future<Output = Result<Vec<RunSummary>, SendableError>> + Send;

    /// Update the status and output of a specific run.
    fn update_run_status(
        &self,
        run_id: i64,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Append a log chunk to an active run.
    fn append_run_chunk(
        &self,
        run_id: i64,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<RunChunk, SendableError>> + Send;

    /// Fetch log chunks for a run with pagination.
    fn fetch_run_chunks(
        &self,
        run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RunChunk>, SendableError>> + Send;

    /// Associate a new artifact with a run.
    fn add_run_artifact(
        &self,
        run_id: i64,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<RunArtifact, SendableError>> + Send;

    /// Fetch all artifacts produced by a specific run.
    fn fetch_run_artifacts(
        &self,
        run_id: i64,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;

    /// Fetch every artifact across all runs, most-recent first.
    fn fetch_all_artifacts(
        &self,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;

    /// Fetch a single artifact by its identifier.
    fn fetch_artifact(
        &self,
        artifact_id: i64,
    ) -> impl Future<Output = Result<Option<RunArtifact>, SendableError>> + Send;

    /// Create or update a workflow definition.
    fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> impl Future<Output = Result<WorkflowDefinition, SendableError>> + Send;

    /// Fetch all workflow definitions.
    fn fetch_workflows(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkflowDefinition>, SendableError>> + Send;

    /// Fetch a workflow definition by its identifier.
    fn fetch_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;

    /// Fetch a workflow definition by its unique display name.
    fn fetch_workflow_by_name(
        &self,
        name: String,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;

    /// Delete a workflow and its associated metadata.
    fn delete_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create or update a workflow trigger.
    fn upsert_workflow_trigger(
        &self,
        trigger: &WorkflowTrigger,
    ) -> impl Future<Output = Result<WorkflowTrigger, SendableError>> + Send;

    /// Fetch all triggers for a workflow definition.
    fn fetch_workflow_triggers(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowTrigger>, SendableError>> + Send;

    /// Fetch a workflow trigger by identifier.
    fn fetch_workflow_trigger(
        &self,
        trigger_id: i64,
    ) -> impl Future<Output = Result<Option<WorkflowTrigger>, SendableError>> + Send;

    /// Delete a workflow trigger.
    fn delete_workflow_trigger(
        &self,
        trigger_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch enabled triggers that should fire at or before the provided instant.
    fn fetch_due_workflow_triggers(
        &self,
        now: DateTime<Utc>,
    ) -> impl Future<Output = Result<Vec<WorkflowTrigger>, SendableError>> + Send;

    /// Update the next execution instant for a workflow trigger.
    fn update_workflow_trigger_next_execution(
        &self,
        trigger_id: i64,
        next_execution: Option<DateTime<Utc>>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Atomically fire due cron triggers and return the workflow runs created by this claim.
    fn claim_due_workflow_trigger_firings(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Create a new instance of a workflow.
    fn create_workflow_run(
        &self,
        workflow_id: i64,
        workflow_snapshot: WorkflowDefinition,
        parameters: Value,
        state: Value,
        name: Option<String>,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;

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
        workflow_run_id: i64,
        scheduler_id: String,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Release a workflow run claim held by a scheduler.
    fn release_workflow_run_claim(
        &self,
        workflow_run_id: i64,
        scheduler_id: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch recent workflow runs across all workflow definitions.
    fn fetch_recent_workflow_runs(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Fetch all runs for a specific workflow definition.
    fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Fetch workflow runs by display name, optionally restricted to open runs.
    fn fetch_workflow_runs_by_name(
        &self,
        name: String,
        open_only: bool,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Update the top-level status of a workflow run.
    fn update_workflow_run_status(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Set or clear the user-facing display name of a workflow run.
    fn set_workflow_run_name(
        &self,
        workflow_run_id: i64,
        name: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch a workflow run summary by its identifier.
    fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> impl Future<Output = Result<Option<WorkflowRun>, SendableError>> + Send;

    /// Create a new node execution record within a workflow run.
    fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: String,
        parameters: Value,
    ) -> impl Future<Output = Result<WorkflowNodeRun, SendableError>> + Send;

    /// Update the status and state of a specific node execution.
    #[allow(clippy::too_many_arguments)]
    fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        attempt: Option<i64>,
        parameters: Option<Value>,
        output_json: Option<Value>,
        state: Option<Value>,
        transition_reason: Option<String>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch all node execution records for a workflow run.
    fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRun>, SendableError>> + Send;

    /// Fetch a node execution record by its identifier.
    fn fetch_workflow_node_run(
        &self,
        workflow_node_run_id: i64,
    ) -> impl Future<Output = Result<Option<WorkflowNodeRun>, SendableError>> + Send;

    /// Append a log chunk to a workflow node run.
    fn append_workflow_node_run_chunk(
        &self,
        workflow_node_run_id: i64,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<WorkflowNodeRunChunk, SendableError>> + Send;

    /// Fetch log chunks for a workflow node run with pagination.
    fn fetch_workflow_node_run_chunks(
        &self,
        workflow_node_run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunChunk>, SendableError>> + Send;

    /// Associate an artifact with a workflow node run.
    fn add_workflow_node_run_artifact(
        &self,
        workflow_node_run_id: i64,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<WorkflowNodeRunArtifact, SendableError>> + Send;

    /// Fetch artifacts for a workflow node run.
    fn fetch_workflow_node_run_artifacts(
        &self,
        workflow_node_run_id: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRunArtifact>, SendableError>> + Send;

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
        workflow_run_id: i64,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<OrchestrationEvent>, SendableError>> + Send;

    /// Enqueue a state-machine node for scheduler processing.
    fn enqueue_ready_node(
        &self,
        event: NewOrchestrationEvent,
        node_id: String,
        ready_at: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

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
        ready_node_id: i64,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Mark a claimed ready-node row complete.
    fn complete_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Fetch ready-node rows still pending drive (uncompleted and not currently claimed), so the
    /// web service can announce them on the wake channel. Includes future `ready_at` rows.
    fn fetch_pending_ready_nodes(
        &self,
        now: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ReadyNodeRecord>, SendableError>> + Send;

    /// Claim a single ready-node row by id for drive, leasing it to `scheduler_id`.
    fn claim_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
    ) -> impl Future<Output = Result<Option<ReadyNodeRecord>, SendableError>> + Send;

    /// Release a claimed ready-node row back to the queued state so it can be re-driven.
    fn release_ready_node(
        &self,
        ready_node_id: i64,
        scheduler_id: String,
    ) -> impl Future<Output = Result<bool, SendableError>> + Send;

    /// Create or update a generic catalog item.
    fn upsert_catalog_item(
        &self,
        item: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch catalog items, optionally filtered by type.
    fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single catalog item by its URI.
    fn fetch_catalog_item(
        &self,
        uri: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Create a new record in a generic orchestration table.
    fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Update an existing orchestration record.
    fn update_automation_record(
        &self,
        record_type: String,
        record_id: i64,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch orchestration records with optional filters.
    fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<i64>,
        external_item_id: Option<i64>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;

    /// Fetch a single orchestration record by its identifier.
    fn fetch_automation_record(
        &self,
        record_type: String,
        record_id: i64,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Store a result for an idempotency key.
    fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;

    /// Fetch the result for an idempotency key if it exists.
    fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    /// Store an action dispatch intent for durable scheduler recovery.
    fn enqueue_action_dispatch(
        &self,
        dedupe_key: String,
        command: ActionCommand,
    ) -> impl Future<Output = Result<ActionDispatchRecord, SendableError>> + Send;

    /// Fetch unpublished action dispatch intents.
    fn fetch_pending_action_dispatches(
        &self,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ActionDispatchRecord>, SendableError>> + Send;

    /// Claim unpublished action dispatch intents for one publisher.
    fn claim_pending_action_dispatches(
        &self,
        scheduler_id: String,
        now: DateTime<Utc>,
        lease_until: DateTime<Utc>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<ActionDispatchRecord>, SendableError>> + Send;

    /// Mark an action dispatch as successfully published.
    fn mark_action_dispatch_published(
        &self,
        dispatch_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Record a failed action dispatch publish attempt.
    fn mark_action_dispatch_failed(
        &self,
        dispatch_id: i64,
        error: String,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Persist a notification record.
    fn create_notification(
        &self,
        notification: &NewNotification,
    ) -> impl Future<Output = Result<Notification, SendableError>> + Send;

    /// Fetch notifications, optionally only unread, most-recent first.
    fn fetch_notifications(
        &self,
        unread_only: bool,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<Notification>, SendableError>> + Send;

    /// Mark a notification as read; returns the updated row.
    fn mark_notification_read(
        &self,
        notification_id: i64,
    ) -> impl Future<Output = Result<Option<Notification>, SendableError>> + Send;

    /// Mark all unread notifications as read; returns the number updated.
    fn mark_all_notifications_read(
        &self,
    ) -> impl Future<Output = Result<u64, SendableError>> + Send;
}
