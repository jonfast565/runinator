use chrono::{DateTime, Utc};
use runinator_models::runs::{NewRunArtifact, NewRunChunk, RunStatus};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Payload sent to the Runinator web service when recording a task run.
#[derive(Debug, Clone, Serialize)]
pub struct TaskRunPayload {
    pub task_id: i64,
    pub started_at: DateTime<Utc>,
    pub duration_ms: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStatusPayload {
    pub status: RunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub type RunChunkPayload = NewRunChunk;
pub type RunArtifactPayload = NewRunArtifact;
