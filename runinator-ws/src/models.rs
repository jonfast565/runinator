use chrono::{DateTime, Utc};

use runinator_models::{
    core::{ScheduledTask, TaskRun},
    runs::{RunArtifact, RunChunk, RunStatus, RunSummary},
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowRun, WorkflowStepRun},
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
    WorkflowStepRun(WorkflowStepRun),
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
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusQuery {
    pub status: Option<RunStatus>,
    pub workflow_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowRunStatusRequest {
    pub status: RunStatus,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowStepRunRequest {
    pub step_id: String,
    #[serde(default)]
    pub parameters: Value,
}

#[derive(Debug, Deserialize)]
pub struct WorkflowStepRunStatusRequest {
    pub status: RunStatus,
    #[serde(default)]
    pub task_run_id: Option<i64>,
    #[serde(default)]
    pub attempt: Option<i64>,
    #[serde(default)]
    pub parameters: Option<Value>,
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkflowRunResponse {
    pub run: WorkflowRun,
    pub steps: Vec<WorkflowStepRun>,
}
