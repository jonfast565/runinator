use crate::value::Value;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

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
    pub run_id: Option<Uuid>,
    pub action_name: String,
    pub action_function: String,
    #[serde(default)]
    pub parameters: Value,
    pub timeout_secs: i64,
    pub artifact_dir: String,
    pub events_jsonl_path: String,
    /// the node's resolved `.idempotent(key: ...)` value, when it declared one. providers with native
    /// idempotency (stripe-style request keys) should pass it to the upstream API so a redelivery the
    /// platform cannot absorb still lands once. `None` for non-idempotent actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idempotency_key: Option<String>,
    /// Worker-resolved path for a currently fenced workspace-affined effect. The engine validates
    /// the opaque affinity before dispatch and the worker guarantees this path remains beneath its
    /// configured workspace root. Providers never receive orchestration ownership details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_path: Option<String>,
}

impl ProviderExecutionRequest {
    /// path the host touches to request cooperative cancellation across the plugin ffi boundary.
    /// derived as a sibling of `events_jsonl_path` (the per-run work dir) so abi-2 plugins can locate
    /// it without a new wire field; `None` when no events path is set (unit tests bypassing a worker).
    pub fn cancel_signal_path(&self) -> Option<std::path::PathBuf> {
        if self.events_jsonl_path.is_empty() {
            return None;
        }
        std::path::Path::new(&self.events_jsonl_path)
            .parent()
            .map(|parent| parent.join("cancel.signal"))
    }
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

/// Input sent by an operator to a provider-owned terminal session. The worker routes these
/// messages to the exact in-flight effect; providers that do not expose a terminal simply never
/// take the receiver from their event sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderTerminalControl {
    Input { data: String },
    Resize { cols: u16, rows: u16 },
    Eof,
}

impl crate::validation::Validate for ProviderTerminalControl {
    fn validate(&self) -> Result<(), crate::validation::ValidationError> {
        match self {
            Self::Input { data } if data.len() > 65_536 => {
                Err(crate::validation::ValidationError::new(
                    "data",
                    "terminal input chunks may not exceed 64 KiB",
                ))
            }
            Self::Resize { cols: 0, .. } => Err(crate::validation::ValidationError::new(
                "cols",
                "must be greater than zero",
            )),
            Self::Resize { rows: 0, .. } => Err(crate::validation::ValidationError::new(
                "rows",
                "must be greater than zero",
            )),
            _ => Ok(()),
        }
    }
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
