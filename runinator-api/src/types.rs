use chrono::{DateTime, Utc};
use serde::Serialize;

/// Payload sent to the Runinator web service when recording a task run.
#[derive(Debug, Clone, Serialize)]
pub struct TaskRunPayload {
    pub task_id: i64,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
