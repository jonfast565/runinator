use chrono::{DateTime, Utc};
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    web::TaskResponse,
};
use serde::{Deserialize, Serialize};

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
}

#[derive(Debug, Deserialize)]
pub struct TaskRunRequest {
    pub task_id: i64,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    pub _message: Option<String>,
}
