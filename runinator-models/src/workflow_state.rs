// typed representations of the workflow run `state` blob and node-run state/output payloads.
//
// the scheduler manipulates these as structs and converts to/from the dynamic `Value` carriers
// (workflow_run.state, workflow_node_run.state, output_json) only at the persistence boundary via
// `runinator_comm::WireCodec`. the web service still owns the same wire shapes, so these structs
// mirror the keys it reads and writes. unmodeled keys round-trip through `#[serde(flatten)]` bags.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::workflows::WorkflowNodeKind;

/// typed view of `workflow_run.state`: a container of named control-flow frames plus user bags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowRunState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ControlFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugFrame>,
    #[serde(rename = "loop", default, skip_serializing_if = "Option::is_none")]
    pub loop_frame: Option<LoopFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parallel: Option<ParallelFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<MapFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub race: Option<RaceFrame>,
    #[serde(rename = "try", default, skip_serializing_if = "Option::is_none")]
    pub try_frame: Option<TryFrame>,
    /// dynamic per-run metadata bag accumulated by config nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_metadata: Option<Value>,
    /// preserves any keys not modeled above (e.g. wait/subflow node snapshots mirrored into state).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `state.control` bookkeeping.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlFrame {
    #[serde(default)]
    pub pause_requested: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `state.debug` bookkeeping pushed to the debugger UI. user-owned fields (mode, breakpoints)
/// survive alongside the runtime-populated fields.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugFrame {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub step_requested: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breakpoints: Vec<String>,
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
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// `state.loop` iteration bookkeeping for a loop body. fields default so a transient `{}` marker
/// (written when a loop body re-enters the loop node) deserializes without error.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LoopFrame {
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub item: Value,
    #[serde(default)]
    pub return_to: String,
}

/// `state.parallel` fan-out bookkeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelFrame {
    pub node_id: String,
    #[serde(default)]
    pub remaining: Vec<String>,
}

/// `state.map` iteration bookkeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapFrame {
    pub node_id: String,
    pub target: String,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub outputs: Vec<Value>,
    #[serde(default = "default_concurrency")]
    pub concurrency: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
}

fn default_concurrency() -> i64 {
    1
}

/// `state.race` fan-out bookkeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceFrame {
    pub node_id: String,
    #[serde(default)]
    pub remaining: Vec<String>,
}

/// `state.try` / try node-run phase bookkeeping.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TryFrame {
    pub node_id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_status: Option<crate::workflows::WorkflowStatus>,
}

impl ParallelFrame {
    /// pop the head branch off `remaining`, leaving the rest for the next visit.
    pub fn pop_remaining(&mut self) -> Option<String> {
        if self.remaining.is_empty() {
            return None;
        }
        Some(self.remaining.remove(0))
    }
}

impl RaceFrame {
    /// pop the head branch off `remaining`, leaving the rest for the next visit.
    pub fn pop_remaining(&mut self) -> Option<String> {
        if self.remaining.is_empty() {
            return None;
        }
        Some(self.remaining.remove(0))
    }
}

// node-run `state` snapshots (workflow_node_run.state).

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

/// subflow node-run state, also mirrored into output for fire-and-forget links. only
/// `subflow_run_id` is required; the rest default so a partial snapshot still deserializes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubflowState {
    pub subflow_run_id: i64,
    #[serde(default)]
    pub subflow_workflow_id: i64,
    #[serde(default)]
    pub run_name: Option<String>,
    #[serde(default)]
    pub reused: bool,
}

/// approval node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalState {
    pub approval: Value,
    pub approval_id: Option<i64>,
}

// node output payloads (serialized into the output_json carrier).

/// loop node iteration output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopOutput {
    pub index: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    pub has_next: bool,
    pub count: usize,
}

/// parallel node fan-out output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelOutput {
    pub branches: Vec<String>,
}

/// map node completion output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapOutput {
    pub count: usize,
    pub outputs: Vec<Value>,
}

/// race node winner output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaceOutput {
    pub winner: String,
}

/// switch node target output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchOutput {
    pub target: Option<String>,
}

/// emit node output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmitOutput {
    pub event_type: Option<String>,
    pub data: Value,
}

/// config node output summarizing the applied name/metadata patch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConfigSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

/// join node satisfaction output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JoinOutput {
    pub wait_for: Vec<String>,
    pub mode: String,
}

/// subflow completion/failure/timeout output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubflowOutcome {
    pub subflow_run_id: i64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<Value>,
}

/// worker fallback status output when a provider does not supply its own output_json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskStatusOutput {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<i64>,
    pub message: Option<String>,
}

/// output recorded when a node is skipped.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkippedOutput {
    pub skipped: bool,
    pub node_id: String,
}

/// the `workflow` entry injected into the template-evaluation scope.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContextHeader {
    pub run_id: i64,
    pub workflow_id: i64,
    pub state: Value,
}

/// idempotency-key record stored for action nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionIdempotencyRecord {
    pub workflow_node_run_id: i64,
}

/// automation record payload posted when an approval node parks a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRecord {
    pub workflow_run_id: i64,
    pub node_id: String,
    pub approval_type: String,
    pub prompt: String,
    pub status: String,
    pub provider: String,
    pub resource_type: String,
    pub external_id: String,
    pub metadata: Value,
}
