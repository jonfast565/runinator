use crate::value::Value;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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
    /// Normalized accounting metadata retained by the worker and attached to the terminal result.
    /// It is never exposed as workflow output.
    AiUsage {
        usage: crate::ai_usage::AiUsage,
    },
    /// A structured, non-terminal provider event. This keeps long-lived agent and other
    /// streaming providers observable without treating provider-specific payloads as workflow
    /// completion output.
    Progress {
        kind: String,
        #[serde(default)]
        payload: Value,
    },
    TerminalInteraction {
        interaction: TerminalInteraction,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalInteractionState {
    InputRequired,
    InputAccepted,
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

mod task_execution_result;
pub use task_execution_result::TaskExecutionResult;

mod new_run_chunk;
pub use new_run_chunk::NewRunChunk;

mod new_run_artifact;
pub use new_run_artifact::NewRunArtifact;

mod provider_execution_request;
pub use provider_execution_request::ProviderExecutionRequest;

mod materialized_credential_injections;
pub use materialized_credential_injections::MaterializedCredentialInjections;

mod provider_execution_response;
pub use provider_execution_response::ProviderExecutionResponse;

mod terminal_interaction;
pub use terminal_interaction::TerminalInteraction;
