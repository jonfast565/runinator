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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pause_on_failure: bool,
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

/// one live loop on a cursor: which loop node, and where that loop is.
///
/// the frame is authoritative. deriving the index by counting the loop node's succeeded runs made
/// an inner loop count every outer lap's runs as its own, so on the second outer pass it was
/// already past its item count and exhausted without running its body once.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoopFrame {
    /// the loop node this frame belongs to; nested loops keep one frame each, keyed by this.
    #[serde(default)]
    pub node_id: String,
    /// the iteration whose body is running now, zero-based.
    #[serde(default)]
    pub index: i64,
    /// the collection snapshot resolved when this loop was entered.
    #[serde(default)]
    pub items: Vec<Value>,
    /// body outputs completed before the current lap, in item order.
    #[serde(default)]
    pub results: Vec<Value>,
    /// this loop's own node run for the current lap. anything this cursor records after it belongs
    /// to that lap's body, which is what makes `LoopOutput.last` body-scoped rather than run-wide.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_node_run_id: Option<Uuid>,
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
