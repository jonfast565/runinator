use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::types::RuninatorType;
use crate::value::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutput {
    pub index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    pub has_next: bool,
    pub count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelOutput {
    pub branches: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapOutput {
    pub count: usize,
    pub outputs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceOutput {
    pub winner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchOutput {
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinOutput {
    pub wait_for: Vec<String>,
    pub mode: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubflowOutcome {
    pub subflow_run_id: Uuid,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusOutput {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedOutput {
    pub skipped: bool,
    pub node_id: String,
}

/// the `workflow` entry injected into the template-evaluation scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContextHeader {
    pub run_id: Uuid,
    pub workflow_id: Uuid,
    pub state: Value,
}

impl WorkflowContextHeader {
    pub fn runinator_type() -> RuninatorType {
        RuninatorType::structure([
            ("run_id", RuninatorType::String),
            ("workflow_id", RuninatorType::String),
            ("state", RuninatorType::Any),
        ])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionIdempotencyRecord {
    pub workflow_node_run_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub approval_type: String,
    pub prompt: String,
    pub status: String,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub metadata: Value,
}
