use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    TimedOut,
    Canceled,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Queued => "queued",
            RunStatus::Running => "running",
            RunStatus::Succeeded => "succeeded",
            RunStatus::Failed => "failed",
            RunStatus::TimedOut => "timed_out",
            RunStatus::Canceled => "canceled",
        }
    }
}

impl TryFrom<&str> for RunStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        match value {
            "queued" => Ok(RunStatus::Queued),
            "running" => Ok(RunStatus::Running),
            "succeeded" => Ok(RunStatus::Succeeded),
            "failed" => Ok(RunStatus::Failed),
            "timed_out" => Ok(RunStatus::TimedOut),
            "canceled" => Ok(RunStatus::Canceled),
            other => Err(format!("Unknown run status '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    #[serde(default)]
    pub parameters: Value,
    #[serde(default = "default_trigger")]
    pub trigger: String,
    #[serde(default)]
    pub workflow_run_id: Option<i64>,
    #[serde(default)]
    pub workflow_node_id: Option<String>,
}

fn default_trigger() -> String {
    "api".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub id: i64,
    pub task_id: i64,
    pub status: RunStatus,
    pub parameters: Value,
    pub output_json: Option<Value>,
    pub message: Option<String>,
    pub trigger: String,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub workflow_run_id: Option<i64>,
    pub workflow_node_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunChunk {
    pub id: i64,
    pub run_id: i64,
    pub sequence: i64,
    pub stream: String,
    pub content: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunArtifact {
    pub id: i64,
    pub run_id: i64,
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    pub metadata: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskExecutionResult {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    pub output_json: Option<Value>,
    #[serde(default)]
    pub chunks: Vec<NewRunChunk>,
    #[serde(default)]
    pub artifacts: Vec<NewRunArtifact>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRunChunk {
    pub stream: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRunArtifact {
    pub name: String,
    pub mime_type: String,
    pub size_bytes: i64,
    pub uri: String,
    #[serde(default)]
    pub metadata: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderExecutionRequest {
    pub task_id: Option<i64>,
    pub run_id: Option<i64>,
    pub action_name: String,
    pub action_function: String,
    #[serde(default)]
    pub parameters: Value,
    pub timeout_secs: i64,
    pub artifact_dir: String,
    pub events_jsonl_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderExecutionResponse {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_json: Option<Value>,
    #[serde(default)]
    pub chunks: Vec<NewRunChunk>,
    #[serde(default)]
    pub artifacts: Vec<NewRunArtifact>,
}

impl From<ProviderExecutionResponse> for TaskExecutionResult {
    fn from(response: ProviderExecutionResponse) -> Self {
        Self {
            message: response.message,
            output_json: response.output_json,
            chunks: response.chunks,
            artifacts: response.artifacts,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderExecutionEvent {
    Chunk {
        stream: String,
        content: String,
    },
    Artifact {
        name: String,
        mime_type: String,
        size_bytes: i64,
        uri: String,
        #[serde(default)]
        metadata: Value,
    },
    Message {
        message: String,
    },
}

impl From<ProviderExecutionEvent> for Option<NewRunChunk> {
    fn from(event: ProviderExecutionEvent) -> Self {
        match event {
            ProviderExecutionEvent::Chunk { stream, content } => {
                Some(NewRunChunk { stream, content })
            }
            _ => None,
        }
    }
}

impl From<ProviderExecutionEvent> for Option<NewRunArtifact> {
    fn from(event: ProviderExecutionEvent) -> Self {
        match event {
            ProviderExecutionEvent::Artifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            } => Some(NewRunArtifact {
                name,
                mime_type,
                size_bytes,
                uri,
                metadata,
            }),
            _ => None,
        }
    }
}
