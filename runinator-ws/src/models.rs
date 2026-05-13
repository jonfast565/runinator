use chrono::{DateTime, Utc};

use runinator_models::{
    core::{ScheduledTask, TaskRun},
    providers::ProviderMetadata,
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowNodeRun, WorkflowRun, WorkflowStatus},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub message: String,
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ApiResponse {
    TaskResponse(TaskResponse),
    ApiError(ApiError),
    ScheduledTaskList(Vec<ScheduledTask>),
    ScheduleTaskRuns(Vec<TaskRun>),
    RunSummary(RunSummary),
    RunList(Vec<RunSummary>),
    RunChunks(Vec<RunChunk>),
    RunArtifacts(Vec<RunArtifact>),
    RunArtifact(RunArtifact),
    Workflow(WorkflowDefinition),
    WorkflowList(Vec<WorkflowDefinition>),
    WorkflowRun(WorkflowRunResponse),
    WorkflowRunList(Vec<WorkflowRun>),
    WorkflowNodeRun(WorkflowNodeRun),
    Provider(ProviderMetadata),
    ProviderList(Vec<ProviderMetadata>),
    JsonValue(Value),
    JsonList(Vec<Value>),
}

#[derive(Debug, Deserialize)]
pub struct TaskRunRequest {
    pub task_id: i64,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub _message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunStatusRequest {
    pub status: RunStatus,
    #[serde(default)]
    pub output_json: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RunStatusQuery {
    pub status: Option<RunStatus>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default)]
    pub debug: bool,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusQuery {
    pub status: Option<WorkflowStatus>,
    pub workflow_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusRequest {
    pub status: WorkflowStatus,
    #[serde(default)]
    pub active_node_id: Option<String>,
    #[serde(default)]
    pub state: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunRequest {
    pub node_id: String,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowNodeRunStatusRequest {
    pub status: WorkflowStatus,
    #[serde(default)]
    pub task_run_id: Option<i64>,
    #[serde(default)]
    pub attempt: Option<i64>,
    #[serde(default)]
    pub parameters: Option<Value>,
    #[serde(default)]
    pub output_json: Option<Value>,
    #[serde(default)]
    pub state: Option<Value>,
    #[serde(default)]
    pub transition_reason: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
    pub nodes: Vec<WorkflowNodeRun>,
}

#[derive(Debug, Deserialize)]
pub struct CatalogQuery {
    pub item_type: Option<String>,
    pub uri: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct AutomationRecordQuery {
    pub workflow_run_id: Option<i64>,
    pub external_item_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ApprovalResolutionRequest {
    #[serde(default)]
    pub resolved_by: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub output_json: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct IdempotencyRequest {
    pub scope: String,
    pub key: String,
    #[serde(default)]
    pub result: Value,
}

#[derive(Debug, Deserialize)]
pub struct CredentialQuery {
    pub scope: Option<String>,
    pub name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CredentialPutRequest {
    pub scope: String,
    pub name: String,
    pub secret: String,
}

#[derive(Debug, Deserialize)]
pub struct WebhookWakeRequest {
    pub workflow_run_id: i64,
    #[serde(default)]
    pub node_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub state: Value,
    #[serde(default)]
    pub message: Option<String>,
}
