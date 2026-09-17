#[allow(unused_imports)]
use super::*;

pub struct DirectiveResponse {
    pub status: AgentDirectiveStatus,
    pub payload: Value,
    pub message: Option<String>,
}

impl DirectiveResponse {
    pub fn completed(payload: Value) -> Self {
        Self {
            status: AgentDirectiveStatus::Completed,
            payload,
            message: None,
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: AgentDirectiveStatus::Unsupported,
            payload: Value::Null,
            message: Some(message.into()),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            status: AgentDirectiveStatus::Failed,
            payload: Value::Null,
            message: Some(message.into()),
        }
    }
}
