use std::future::Future;

use chrono::{DateTime, Utc};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunArtifact, RunChunk, RunStatus, RunSummary},
    workflows::{WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde_json::Value;

// doing something like this would make the code look more readable:
// but alas Rust kills us every time
// pub type DatabaseAsyncReturn<T> = impl Future<Output = Result<T, SendableError>> + Send;

// NOTE: Ensure anything that implements this trait cannot contain a reference
// otherwise, this is breaking major rules
pub trait DatabaseImpl: Send + Sync + 'static {
    fn run_init_scripts(
        &self,
        paths: &Vec<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn upsert_task(
        &self,
        task: &ScheduledTask,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn delete_task(&self, task_id: i64) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn fetch_all_tasks(
        &self,
    ) -> impl Future<Output = Result<Vec<ScheduledTask>, SendableError>> + Send;
    fn fetch_task_by_id(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<Option<ScheduledTask>, SendableError>> + Send;
    fn fetch_task_runs(
        &self,
        start: i64,
        end: i64,
    ) -> impl Future<Output = Result<Vec<TaskRun>, SendableError>> + Send;
    fn update_task_next_execution(
        &self,
        task: &ScheduledTask,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn log_task_run(
        &self,
        task_id: i64,
        start_time: DateTime<Utc>,
        duration_ms: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn request_immediate_run(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn clear_immediate_run(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;

    fn create_task_run(
        &self,
        task_id: i64,
        parameters: Value,
        trigger: String,
        workflow_run_id: Option<i64>,
        workflow_step_id: Option<String>,
    ) -> impl Future<Output = Result<RunSummary, SendableError>> + Send;
    fn fetch_run(
        &self,
        run_id: i64,
    ) -> impl Future<Output = Result<Option<RunSummary>, SendableError>> + Send;
    fn fetch_runs_for_task(
        &self,
        task_id: i64,
    ) -> impl Future<Output = Result<Vec<RunSummary>, SendableError>> + Send;
    fn fetch_runs_by_status(
        &self,
        status: RunStatus,
    ) -> impl Future<Output = Result<Vec<RunSummary>, SendableError>> + Send;
    fn update_run_status(
        &self,
        run_id: i64,
        status: RunStatus,
        output_json: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn append_run_chunk(
        &self,
        run_id: i64,
        chunk: &NewRunChunk,
    ) -> impl Future<Output = Result<RunChunk, SendableError>> + Send;
    fn fetch_run_chunks(
        &self,
        run_id: i64,
        cursor: Option<i64>,
        limit: i64,
    ) -> impl Future<Output = Result<Vec<RunChunk>, SendableError>> + Send;
    fn add_run_artifact(
        &self,
        run_id: i64,
        artifact: &NewRunArtifact,
    ) -> impl Future<Output = Result<RunArtifact, SendableError>> + Send;
    fn fetch_run_artifacts(
        &self,
        run_id: i64,
    ) -> impl Future<Output = Result<Vec<RunArtifact>, SendableError>> + Send;
    fn fetch_artifact(
        &self,
        artifact_id: i64,
    ) -> impl Future<Output = Result<Option<RunArtifact>, SendableError>> + Send;
    fn upsert_workflow(
        &self,
        workflow: &WorkflowDefinition,
    ) -> impl Future<Output = Result<WorkflowDefinition, SendableError>> + Send;
    fn fetch_workflows(
        &self,
    ) -> impl Future<Output = Result<Vec<WorkflowDefinition>, SendableError>> + Send;
    fn fetch_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<Option<WorkflowDefinition>, SendableError>> + Send;
    fn delete_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn create_workflow_run(
        &self,
        workflow_id: i64,
        parameters: Value,
    ) -> impl Future<Output = Result<WorkflowRun, SendableError>> + Send;
    fn fetch_workflow_runs_by_status(
        &self,
        status: WorkflowStatus,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;
    fn fetch_workflow_runs_for_workflow(
        &self,
        workflow_id: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowRun>, SendableError>> + Send;
    fn update_workflow_run_status(
        &self,
        workflow_run_id: i64,
        status: WorkflowStatus,
        active_node_id: Option<String>,
        state: Option<Value>,
        message: Option<String>,
    ) -> impl Future<Output = Result<(), SendableError>> + Send;
    fn fetch_workflow_run(
        &self,
        workflow_run_id: i64,
    ) -> impl Future<Output = Result<Option<WorkflowRun>, SendableError>> + Send;
    fn create_workflow_node_run(
        &self,
        workflow_run_id: i64,
        node_id: String,
        parameters: Value,
    ) -> impl Future<Output = Result<WorkflowNodeRun, SendableError>> + Send;
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
    fn fetch_workflow_node_runs(
        &self,
        workflow_run_id: i64,
    ) -> impl Future<Output = Result<Vec<WorkflowNodeRun>, SendableError>> + Send;

    fn upsert_catalog_item(
        &self,
        item: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;
    fn fetch_catalog_items(
        &self,
        item_type: Option<String>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;
    fn fetch_catalog_item(
        &self,
        uri: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    fn create_automation_record(
        &self,
        record_type: String,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;
    fn update_automation_record(
        &self,
        record_type: String,
        record_id: i64,
        record: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;
    fn fetch_automation_records(
        &self,
        record_type: String,
        workflow_run_id: Option<i64>,
        external_item_id: Option<i64>,
    ) -> impl Future<Output = Result<Vec<Value>, SendableError>> + Send;
    fn fetch_automation_record(
        &self,
        record_type: String,
        record_id: i64,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;

    fn put_idempotency_key(
        &self,
        scope: String,
        key: String,
        result: Value,
    ) -> impl Future<Output = Result<Value, SendableError>> + Send;
    fn fetch_idempotency_key(
        &self,
        scope: String,
        key: String,
    ) -> impl Future<Output = Result<Option<Value>, SendableError>> + Send;
}
