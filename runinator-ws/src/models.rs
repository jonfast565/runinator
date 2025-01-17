use runinator_models::{core::{ScheduledTask, TaskRun}, web::TaskResponse};
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct Empty {}

#[derive(Debug, Serialize)]
pub struct ApiError {
    pub message: String
}

#[derive(Serialize)]
#[serde(untagged)]
pub enum ApiResponse {
    TaskResponse(TaskResponse),
    ApiError(ApiError),
    ScheduledTaskList(Vec<ScheduledTask>),
    ScheduleTaskRuns(Vec<TaskRun>),
}