use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{Map, Value};
use crate::workflows::{WorkflowNodeKind, WorkflowStatus};

/// `state.subflow_parent`: the parent run and node a child run reports back to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubflowParent {
    pub run_id: Uuid,
    pub node_id: String,
}

/// one entry of `state.event_sources`: an inbound event parked for an event_source node.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct EventSourceEntry {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_event: Option<Value>,
}

/// `state.control` bookkeeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFrame {
    #[serde(default)]
    pub pause_requested: bool,
    #[serde(flatten)]
    pub extra: Map,
}

/// debug step granularity: pause before every node, or only at breakpoints.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DebugMode {
    #[default]
    StepAll,
    Breakpoints,
}

/// `state.debug` configuration plus the primary cursor's runtime mirror.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugFrame {
    #[serde(flatten)]
    pub config: DebugConfig,
    #[serde(flatten)]
    pub runtime: DebugRuntime,
    #[serde(flatten)]
    pub extra: Map,
}

/// user-owned debug configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DebugMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breakpoints: Vec<String>,
}

/// reducer-owned per-cursor debug runtime state.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DebugRuntime {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub step_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub one_shot_breakpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_node_kind: Option<WorkflowNodeKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_json: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_output_json: Option<Value>,
}

/// `state.loop` iteration bookkeeping for a loop body.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoopFrame {
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub item: Value,
    #[serde(default)]
    pub return_to: String,
}

/// `state.map` parent fan-out bookkeeping or child item binding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapFrame {
    pub node_id: String,
    pub target: String,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default = "default_concurrency")]
    pub concurrency: i64,
    #[serde(default)]
    pub next_index: i64,
    #[serde(default)]
    pub in_flight: Vec<MapChild>,
    #[serde(default)]
    pub results: Vec<Value>,
    #[serde(default)]
    pub done: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    #[serde(default)]
    pub index: i64,
}

/// one in-flight map item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapChild {
    pub index: i64,
    pub child_run_id: Uuid,
}

/// child-run marker stored under `state.map_child`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapChildState {
    pub stop_node: String,
    pub index: i64,
    pub item: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
}

fn default_concurrency() -> i64 {
    1
}

/// `state.compensation` saga-rollback bookkeeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompensationFrame {
    #[serde(default)]
    pub remaining: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<Uuid>,
}

/// `state.try` phase bookkeeping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TryFrame {
    pub node_id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_status: Option<WorkflowStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_output: Option<Value>,
}
