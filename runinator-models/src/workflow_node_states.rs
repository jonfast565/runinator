use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::Value;

/// wait node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitState {
    pub deadline_unix: i64,
    pub status: String,
}

/// wait node output recorded when the deadline elapses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitElapsedOutput {
    pub deadline_unix: i64,
}

/// output node output recorded when an output node publishes its payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    pub data: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Value>,
}

/// input node state while it waits for a user response in the UI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputState {
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<Uuid>,
}

/// subflow node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubflowState {
    pub subflow_run_id: Uuid,
    #[serde(default)]
    pub subflow_workflow_id: Uuid,
    #[serde(default)]
    pub run_name: Option<String>,
    #[serde(default)]
    pub reused: bool,
}

/// approval node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalState {
    pub approval: Value,
    pub approval_id: Option<Uuid>,
}

/// gate node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub gate_id: Option<Uuid>,
    #[serde(default)]
    pub deadline_unix: Option<i64>,
    pub poll_interval: i64,
}

/// signal node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalState {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_key: Option<String>,
}
