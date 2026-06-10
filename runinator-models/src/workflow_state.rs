// typed representations of the workflow run `state` blob and node-run state/output payloads.
//
// the scheduler manipulates these as structs and converts to/from the dynamic `Value` carriers
// (workflow_run.state, workflow_node_run.state, output_json) only at the persistence boundary via
// `runinator_comm::WireCodec`. the web service still owns the same wire shapes, so these structs
// mirror the keys it reads and writes. unmodeled keys round-trip through `#[serde(flatten)]` bags.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::value::{Map, Value};

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
    pub extra: Map,
}

impl WorkflowRunState {
    /// parse a run's `state` blob into the typed container. malformed state collapses to empty.
    pub fn from_state(value: &Value) -> Self {
        serde_json::from_value(value.clone().into()).unwrap_or_default()
    }

    /// serialize back into a `state` blob for persistence.
    pub fn to_state(&self) -> Value {
        serde_json::to_value(self)
            .map(Value::from)
            .unwrap_or(Value::Null)
    }
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
    /// pause before every node.
    #[default]
    StepAll,
    /// pause only at configured breakpoints (or a one-shot cursor).
    Breakpoints,
}

/// `state.debug` bookkeeping pushed to the debugger UI. the frame is split into user-owned
/// configuration ([`DebugConfig`]) and scheduler-owned runtime state ([`DebugRuntime`]); both are
/// flattened so the persisted/wire json stays a single flat `debug` object.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugFrame {
    /// user-owned settings that survive across pauses and steps.
    #[serde(flatten)]
    pub config: DebugConfig,
    /// scheduler-owned state rewritten on each pause/step.
    #[serde(flatten)]
    pub runtime: DebugRuntime,
    /// preserves any debug keys not modeled above.
    #[serde(flatten)]
    pub extra: Map,
}

/// user-owned debug configuration. only the debugger UI writes these.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DebugConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<DebugMode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub breakpoints: Vec<String>,
}

/// scheduler-owned debug runtime state. the scheduler overwrites these as a run pauses and steps.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

/// `state.map` bookkeeping. the parent map node owns the fan-out cursor
/// (`next_index`/`in_flight`/`results`/`done`); a child run carries only the `item`/`index`
/// it is bound to so the body can resolve the map variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapFrame {
    pub node_id: String,
    pub target: String,
    #[serde(default)]
    pub items: Vec<Value>,
    #[serde(default = "default_concurrency")]
    pub concurrency: i64,
    /// parent: next item index to dispatch into a child run.
    #[serde(default)]
    pub next_index: i64,
    /// parent: child runs each executing one item.
    #[serde(default)]
    pub in_flight: Vec<MapChild>,
    /// parent: per-item body output, positional; `Null` until that item completes.
    #[serde(default)]
    pub results: Vec<Value>,
    /// parent: completed item count.
    #[serde(default)]
    pub done: i64,
    /// child: the item bound to this child run (also exposed via the seeded map node-run output).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<Value>,
    /// child: the item index bound to this child run.
    #[serde(default)]
    pub index: i64,
}

/// one in-flight map item: the child run executing it and its item index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapChild {
    pub index: i64,
    pub child_run_id: Uuid,
}

/// child-run marker stored under `state.map_child`: where the body re-enters the map (and must
/// stop), which item is bound, and the captured body output once the child finishes.
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

/// output node output recorded when an output node publishes its payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_type: Option<String>,
    pub data: Value,
}

/// input node state while it waits for a user response in the ui.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputState {
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_id: Option<Uuid>,
}

/// subflow node-run state, also mirrored into output for fire-and-forget links. only
/// `subflow_run_id` is required; the rest default so a partial snapshot still deserializes.
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
    pub subflow_run_id: Uuid,
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
    pub run_id: Uuid,
    pub workflow_id: Uuid,
    pub state: Value,
}

/// idempotency-key record stored for action nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionIdempotencyRecord {
    pub workflow_node_run_id: Uuid,
}

/// automation record payload posted when an approval node parks a run.
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
