use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;

/// Core persistence operations for Runinator.
pub trait DatabaseImpl: Send + Sync + 'static {
    /// Execute initialization scripts for the database.
    fn run_init_scripts(
        &self,
        paths: &Vec<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Add or update a scheduled task.
    fn upsert_task(
        &self,
        task: &ScheduledTask,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Delete a scheduled task by its identifier.
    fn delete_task(&self, task_id: i64) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Fetch all enabled and disabled tasks.
    fn fetch_all_tasks(
        &self,
    ) -> impl Future<Output = Result<Vec<ScheduledTask>, SendableError>> + Send;

    /// Fetch a single task by its identifier.
    fn fetch_task_by_id(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<Option<ScheduledTask>, SendableError>> + Send;

    /// Fetch task execution records within a time range.
    fn fetch_task_runs(
        &self,
        start: i64,
        end: i64,
    ) -> impl Future<Output = Result<Vec<TaskRun>, SendableError>> + Send;

    /// Update the next scheduled execution time for a task.
    fn update_task_next_execution(
        &self,
        task: &ScheduledTask,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Record a completed task execution.
    fn log_task_run(
        &self,
        task_id: i64,
        start_time: DateTime<Utc>,
        duration_ms: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Request a task to be executed as soon as possible.
    fn request_immediate_run(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Clear the immediate run flag for a task.
    fn clear_immediate_run(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create a new task run record.
    fn create_task_run(
        &self,
        task_id: i64,
        parameters: Value,
        trigger: String,
        workflow_run_id: Option<i64>,
        workflow_node_id: Option<String>,
    ) -> impl Future<Output = Result<RunSummary, SendableError>> + Send;

    /// Fetch a run summary by its identifier.
    fn fetch_run(
        &self,
        run_id: i64,
    ) -> impl Future<Output = Result<Option<RunSummary>, SendableError>> + Send;

    /// Fetch all runs associated with a specific task.
    fn fetch_runs_for_task(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<Vec<RunSummary>, SendableError>> + Send;

    /// Fetch runs filtered by their current status.
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

    /// Delete a workflow and its associated metadata.
    fn delete_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    /// Create a new instance of a workflow.
    fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
        state: Value,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;

    /// Fetch workflow runs filtered by status.
    fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;

    /// Fetch all runs for a specific workflow definition.
    fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: i64,
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
    fn update_workflow_node_run(
        &self,
        node_run_id: i64,
        status: WorkflowStatus,
        task_run_id: Option<i64>,
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
}
