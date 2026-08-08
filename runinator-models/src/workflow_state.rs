// typed representations of the workflow run `state` blob and node-run state/output payloads.
//
// the scheduler manipulates these as structs and converts to/from the dynamic `Value` carriers
// (workflow_run.state, workflow_node_run.state, output_json) only at the persistence boundary via
// `runinator_comm::WireCodec`. the web service still owns the same wire shapes, so these structs
// mirror the keys it reads and writes. unmodeled keys round-trip through `#[serde(flatten)]` bags.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::cursor::RunCursor;
use crate::orchestration::GateKind;
use crate::value::{Map, Value};

use crate::workflows::WorkflowNodeKind;

/// the top-level key prefix older runs used for a per-node event_source delivery slot, before those
/// slots were consolidated under [`WorkflowRunState::event_sources`].
const LEGACY_EVENT_SOURCE_PREFIX: &str = "event_source_";

// deserialize a frame tolerantly: a malformed payload becomes `None` rather than failing the parse
// of the whole state blob. these frames were previously read out of the untyped bag with
// `from_wire_value(..).ok()`, and that tolerance is what stops one bad frame from discarding a
// run's entire state.
fn lenient_frame<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let raw = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(raw).ok())
}

/// typed view of `workflow_run.state`: a container of named control-flow frames plus user bags.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowRunState {
    /// where the run is on its track. one entry for a linear run; `parallel`/`race` fan out more.
    /// empty for a run that has not been placed yet, which the reducer seeds on its first drive.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cursors: Vec<RunCursor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub control: Option<ControlFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub debug: Option<DebugFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<MapFrame>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compensation: Option<CompensationFrame>,
    /// set on a child run (subflow or map item) so reaching a terminal can wake the parent node
    /// that started it.
    #[serde(
        default,
        deserialize_with = "lenient_frame",
        skip_serializing_if = "Option::is_none"
    )]
    pub subflow_parent: Option<SubflowParent>,
    /// set on a map fan-out child: the item it is bound to and where its body must stop.
    #[serde(
        default,
        deserialize_with = "lenient_frame",
        skip_serializing_if = "Option::is_none"
    )]
    pub map_child: Option<MapChildState>,
    /// per-node event_source delivery slots, keyed by node id. older runs carried these as dynamic
    /// top-level `event_source_<node_id>` keys; [`WorkflowRunState::from_state`] folds that shape in.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub event_sources: BTreeMap<String, EventSourceEntry>,
    /// dynamic per-run metadata bag accumulated by config nodes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_metadata: Option<Value>,
    /// set once a workflow-level `watch` guard has fired, so it redirects to its handler at most once.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub watch_fired: bool,
    /// preserves any keys not modeled above (e.g. wait/subflow node snapshots mirrored into state).
    #[serde(flatten)]
    pub extra: Map,
}

impl WorkflowRunState {
    /// parse a run's `state` blob into the typed container. malformed state collapses to empty.
    pub fn from_state(value: &Value) -> Self {
        let mut parsed: Self = serde_json::from_value(value.clone().into()).unwrap_or_default();
        parsed.absorb_legacy_event_sources();
        parsed
    }

    /// the cursor with this id, if the run still holds it.
    pub fn cursor(&self, id: Uuid) -> Option<&RunCursor> {
        self.cursors.iter().find(|cursor| cursor.id == id)
    }

    /// mutable access to the cursor with this id.
    pub fn cursor_mut(&mut self, id: Uuid) -> Option<&mut RunCursor> {
        self.cursors.iter_mut().find(|cursor| cursor.id == id)
    }

    /// the first cursor sitting on `node_id`. used to resolve a ready-queue row enqueued before
    /// cursors carried ids.
    pub fn cursor_at(&self, node_id: &str) -> Option<&RunCursor> {
        self.cursors.iter().find(|cursor| cursor.is_at(node_id))
    }

    /// the cursor mirrored into `workflow_runs.active_node_id`, and the one single-threaded
    /// consumers (the debugger, the run detail ui) follow.
    pub fn primary_cursor(&self) -> Option<&RunCursor> {
        self.cursors.first()
    }

    /// place a run that has no cursors yet, returning the seeded cursor's id. a no-op returning the
    /// existing primary when the run is already placed.
    pub fn ensure_cursor(&mut self, node_id: &str) -> Uuid {
        if let Some(cursor) = self.cursors.first() {
            return cursor.id;
        }
        let cursor = RunCursor::at(node_id);
        let id = cursor.id;
        self.cursors.push(cursor);
        id
    }

    /// fork a branch cursor entering `node_id`, attributed to the fan-out node `forked_by`.
    pub fn fork_cursor(&mut self, node_id: &str, forked_by: &str) -> Uuid {
        let cursor = RunCursor::forked(node_id, forked_by);
        let id = cursor.id;
        self.cursors.push(cursor);
        id
    }

    /// drop a cursor that has reached the end of its thread of control. returns whether it was
    /// still there, so a caller can tell a first retirement from a repeat.
    pub fn retire_cursor(&mut self, id: Uuid) -> bool {
        let before = self.cursors.len();
        self.cursors.retain(|cursor| cursor.id != id);
        self.cursors.len() != before
    }

    /// every *real* cursor forked by `forked_by`, for a join deciding whether its branches have all
    /// arrived and for a race retiring its losers.
    ///
    /// speculative cursors are excluded: a debugger "what if" branch must not be able to make a
    /// join think a real branch arrived, nor a race think it has already fanned out.
    pub fn cursors_forked_by(&self, forked_by: &str) -> impl Iterator<Item = &RunCursor> {
        self.cursors.iter().filter(move |cursor| {
            cursor.speculative.is_none()
                && cursor
                    .forked_by
                    .as_deref()
                    .is_some_and(|origin| origin == forked_by)
        })
    }

    /// the real threads of control — everything the run's completion actually depends on.
    pub fn real_cursors(&self) -> impl Iterator<Item = &RunCursor> {
        self.cursors
            .iter()
            .filter(|cursor| !cursor.is_speculative())
    }

    /// is the cursor with this id a debugger "what if" branch?
    pub fn is_speculative(&self, id: Uuid) -> bool {
        self.cursor(id).is_some_and(RunCursor::is_speculative)
    }

    /// `root` plus every speculative cursor transitively forked from it, so an abandoned or failed
    /// fork drains as a unit instead of leaving orphaned children behind.
    pub fn speculative_subtree(&self, root: Uuid) -> Vec<Uuid> {
        let mut found = vec![root];
        let mut index = 0;
        while index < found.len() {
            let parent = found[index];
            index += 1;
            for cursor in &self.cursors {
                let forked_from = cursor
                    .speculative
                    .as_ref()
                    .map(|frame| frame.forked_from_cursor);
                if forked_from == Some(parent) && !found.contains(&cursor.id) {
                    found.push(cursor.id);
                }
            }
        }
        found
    }

    /// `id` plus every speculative cursor it was forked from, walking up to the first real one.
    ///
    /// this is a fork's *history*: a branch forked from another continues that other's exploration,
    /// so the parent's recorded work is part of this branch's past. the descendants in
    /// [`Self::speculative_subtree`] are the opposite relation — divergent continuations, whose work
    /// this branch must not see — which is why draining and visibility use different walks.
    pub fn speculative_ancestry(&self, id: Uuid) -> Vec<Uuid> {
        let mut chain = vec![id];
        let mut current = id;
        // bounded by the cursor count: each step moves to a strictly different cursor, and a cursor
        // already in the chain stops the walk, so a corrupted cycle cannot spin here.
        while let Some(parent) = self
            .cursor(current)
            .and_then(|cursor| cursor.speculative.as_ref())
            .map(|frame| frame.forked_from_cursor)
        {
            if chain.contains(&parent) {
                break;
            }
            chain.push(parent);
            current = parent;
        }
        chain
    }

    /// fork a speculative branch of `from`, entering at `node_id`. returns the new cursor's id, or
    /// `None` when `from` has already been retired.
    pub fn fork_speculative(
        &mut self,
        from: Uuid,
        node_id: &str,
        label: Option<String>,
        context_patch: Value,
    ) -> Option<Uuid> {
        let parent = self.cursor(from)?;
        let cursor = RunCursor::speculative_from(parent, node_id, label, context_patch);
        let id = cursor.id;
        self.cursors.push(cursor);
        Some(id)
    }

    /// the debugger runtime governing one cursor.
    ///
    /// falls back to the run-scoped frame only while *no* cursor carries a runtime of its own —
    /// which is exactly a run persisted before per-cursor debug state, so it resumes intact. once
    /// any cursor has been written, the flat frame is the primary's mirror rather than the run's
    /// state, and a sibling without a runtime is simply not under the debugger.
    pub fn cursor_debug(&self, id: Uuid) -> DebugRuntime {
        if let Some(runtime) = self.cursor(id).and_then(|cursor| cursor.debug.clone()) {
            return runtime;
        }
        if self.cursors.iter().any(|cursor| cursor.debug.is_some()) {
            return DebugRuntime::default();
        }
        self.debug
            .as_ref()
            .map(|frame| frame.runtime.clone())
            .unwrap_or_default()
    }

    /// write one cursor's debugger runtime, then refresh the run-scoped mirror.
    pub fn set_cursor_debug(&mut self, id: Uuid, runtime: DebugRuntime) {
        if let Some(cursor) = self.cursor_mut(id) {
            cursor.debug = Some(runtime);
        }
        self.mirror_primary_debug();
    }

    /// copy the primary cursor's runtime into the flat `debug` object.
    ///
    /// the frame is the wire contract single-position clients read, so it has to follow whichever
    /// cursor `active_node_id` is reporting.
    pub fn mirror_primary_debug(&mut self) {
        let Some(runtime) = self
            .primary_cursor()
            .and_then(|cursor| cursor.debug.clone())
        else {
            return;
        };
        if let Some(frame) = self.debug.as_mut() {
            frame.runtime = runtime;
        }
    }

    /// is every live cursor parked under the debugger?
    ///
    /// this is the whole condition for the run itself being `DebugPaused`: one branch stopping at a
    /// breakpoint leaves the run `Running`, because its siblings are still executing.
    pub fn all_cursors_paused(&self) -> bool {
        !self.cursors.is_empty()
            && self
                .cursors
                .iter()
                .all(|cursor| self.cursor_debug(cursor.id).paused)
    }

    /// is this run a child of another run's node (a subflow child or a map item)? child runs must
    /// not fan out further chained workflows or pipelines — only top-level runs chain.
    pub fn is_child_run(&self) -> bool {
        self.subflow_parent.is_some() || self.map_child.is_some()
    }

    /// the delivery slot an event_source node reads, if one has been stamped.
    pub fn event_source(&self, node_id: &str) -> Option<&EventSourceEntry> {
        self.event_sources.get(node_id)
    }

    /// stamp an inbound event for `node_id`, replacing any undelivered one.
    pub fn deliver_event(&mut self, node_id: &str, event: Value) {
        self.event_sources
            .entry(node_id.to_string())
            .or_default()
            .pending_event = Some(event);
    }

    // fold the older top-level `event_source_<node_id>` keys into `event_sources` on read, so both
    // shapes drive the same code and the next write persists only the consolidated form. an
    // already-consolidated entry wins, since it is the newer of the two.
    fn absorb_legacy_event_sources(&mut self) {
        let legacy = self
            .extra
            .keys()
            .filter(|key| key.starts_with(LEGACY_EVENT_SOURCE_PREFIX))
            .cloned()
            .collect::<Vec<_>>();
        for key in legacy {
            let Some(raw) = self.extra.remove(&key) else {
                continue;
            };
            let Some(node_id) = key.strip_prefix(LEGACY_EVENT_SOURCE_PREFIX) else {
                continue;
            };
            let entry = serde_json::from_value::<EventSourceEntry>(raw.into()).unwrap_or_default();
            self.event_sources
                .entry(node_id.to_string())
                .or_insert(entry);
        }
    }

    /// serialize back into a `state` blob for persistence.
    pub fn to_state(&self) -> Value {
        serde_json::to_value(self)
            .map(Value::from)
            .unwrap_or(Value::Null)
    }
}

#[cfg(test)]
#[path = "workflow_state_tests.rs"]
mod workflow_state_tests;

/// `state.subflow_parent`: the parent run and node a child run reports back to. stamped at child
/// creation by the subflow and map fan-out paths.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubflowParent {
    pub run_id: Uuid,
    pub node_id: String,
}

/// one entry of `state.event_sources`: the slot an inbound event is stamped into for a parked
/// event_source node, consumed on the next drive.
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
    /// pause before every node.
    #[default]
    StepAll,
    /// pause only at configured breakpoints (or a one-shot cursor).
    Breakpoints,
}

/// `state.debug` bookkeeping pushed to the debugger UI. the frame is split into user-owned
/// configuration ([`DebugConfig`]) and reducer-owned runtime state ([`DebugRuntime`]); both are
/// flattened so the persisted/wire json stays a single flat `debug` object.
///
/// the config is run-scoped: a breakpoint is a property of the graph as authored, so every cursor
/// honors the same set. the runtime is **the primary cursor's mirror** — the authoritative copy for
/// each thread of control lives on its own [`crate::cursor::RunCursor`]. a client that only ever
/// showed one position keeps reading this frame unchanged.
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

/// reducer-owned debug runtime state, rewritten as a thread of control pauses and steps.
///
/// this is **per-cursor** state: it lives on [`crate::cursor::RunCursor::debug`], and the copy on
/// [`DebugFrame`] is the primary cursor's mirror, exactly as `workflow_runs.active_node_id` mirrors
/// the primary cursor's position. a run with fan-out has one of these per live branch.
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

/// `state.loop` iteration bookkeeping for a loop body. fields default so a transient `{}` marker
/// (written when a loop body re-enters the loop node) deserializes without error.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct LoopFrame {
    #[serde(default)]
    pub index: i64,
    #[serde(default)]
    pub item: Value,
    #[serde(default)]
    pub return_to: String,
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

/// `state.compensation` saga-rollback bookkeeping. populated when a run reaches a failed terminal
/// while succeeded nodes carry `compensation` actions; the engine unwinds `remaining` in order
/// (already reverse of completion), dispatching one compensation action at a time.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompensationFrame {
    /// origin node ids whose compensations still need to run, in execution order.
    #[serde(default)]
    pub remaining: Vec<String>,
    /// the synthetic compensation node-run currently executing, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_run_id: Option<uuid::Uuid>,
}

/// `state.try` / try node-run phase bookkeeping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TryFrame {
    pub node_id: String,
    pub phase: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_status: Option<crate::workflows::WorkflowStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_output: Option<Value>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<Value>,
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

/// gate node-run state. `deadline_unix` is the optional max-wait cutoff; `poll_interval` is how
/// often the reducer re-checks while the gate stays closed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateState {
    pub gate_id: Option<Uuid>,
    #[serde(default)]
    pub deadline_unix: Option<i64>,
    pub poll_interval: i64,
}

/// signal node-run state. carries the signal name the node is parked on so an inbound delivery can
/// match the right waiting node. an optional resolved `correlation_key` (e.g. a ticket key or PR
/// number) lets an external webhook route to the right parked run without knowing its run id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalState {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_key: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last: Option<Value>,
}

/// parallel node fan-out output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParallelOutput {
    pub branches: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Value>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<Value>,
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

impl WorkflowContextHeader {
    /// the static type of the run context exposed to expressions under the `run` root. single source
    /// for the front-end/runtime type checkers, kept in lockstep with this struct's wire shape: the
    /// uuids serialize as strings and `state` is an arbitrary runtime blob.
    pub fn runinator_type() -> crate::types::RuninatorType {
        use crate::types::RuninatorType;
        RuninatorType::structure([
            ("run_id", RuninatorType::String),
            ("workflow_id", RuninatorType::String),
            // the run state blob is arbitrary and only known at runtime.
            ("state", RuninatorType::Any),
        ])
    }
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

// --- new node state and output types ---

/// one failing assertion in an assert node.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertViolation {
    pub name: String,
    pub message: String,
}

/// assert node output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssertOutput {
    pub passed: bool,
    pub violations: Vec<AssertViolation>,
}

/// transform node output: the resolved binding map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformOutput {
    pub bindings: Value,
}

/// audit node output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditOutput {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Uuid>,
    pub actor: Option<String>,
    pub action: String,
    pub target: Option<String>,
}

/// checkpoint node output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointOutput {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint_id: Option<Uuid>,
}

/// mutex node-run state while the run is parked waiting to acquire.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutexState {
    pub name: String,
    pub poll_interval: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}

/// mutex node output on acquisition or release.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MutexOutput {
    pub name: String,
    pub acquired: bool,
    /// true when this node released the lock (an end-of-section release node).
    #[serde(default)]
    pub released: bool,
}

/// throttle node-run state while parked waiting for a token.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleState {
    pub name: String,
    pub poll_interval: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}

/// throttle node output on admission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThrottleOutput {
    pub name: String,
    pub admitted: bool,
}

/// cooldown node output: whether this pass was short-circuited by the cooldown window.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CooldownOutput {
    pub name: String,
    /// true when the run was completed without entering the body (still inside the window).
    pub skipped: bool,
    /// seconds left in the window when skipped; 0 when the pass proceeded.
    pub remaining_seconds: i64,
}

/// await_workflow node-run state while parked watching runs of a named workflow. matches runs by
/// target workflow id, optionally narrowed to a resolved correlation value and to runs started at or
/// after `since_unix`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitWorkflowState {
    pub workflow_id: Uuid,
    pub workflow_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correlation_value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_unix: Option<i64>,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}

/// await_workflow node output when the satisfaction policy is met.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitWorkflowOutput {
    pub workflow_id: Uuid,
    pub matched_run_ids: Vec<Uuid>,
    pub mode: String,
    pub statuses: Vec<String>,
}

/// debounce node-run state while parked waiting for the trailing window to lapse.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceState {
    pub deadline_unix: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger_key: Option<String>,
}

/// debounce node output when the window lapses with no new trigger.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DebounceOutput {
    pub deadline_unix: i64,
}

/// collect node-run state while parked accumulating items.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectState {
    pub name: String,
    pub items: Vec<Value>,
    pub threshold: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}

/// collect node output when the threshold or timeout is reached.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectOutput {
    pub items: Vec<Value>,
    pub count: usize,
    /// `"threshold"` or `"timeout"`.
    pub reason: String,
}

/// barrier node-run state while parked waiting for N arrivals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierState {
    pub name: String,
    pub expected_count: i64,
    pub arrivals: Vec<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
}

/// barrier node output when the last arrival completes the rendezvous.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BarrierOutput {
    pub name: String,
    pub arrivals: Vec<Uuid>,
}

/// circuit_breaker node-run state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerState {
    pub name: String,
    /// `"closed"`, `"open"`, or `"half_open"`.
    pub circuit_state: String,
}

/// circuit_breaker node output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CircuitBreakerOutput {
    pub name: String,
    pub circuit_state: String,
    pub tripped: bool,
}

/// event_source node-run state while subscribed to the event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSourceState {
    pub event_type: String,
    pub events_processed: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deadline_unix: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_events: Option<i64>,
}

/// gate row payload the reducer inserts when a gate node first parks a run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateRecord {
    pub workflow_run_id: Uuid,
    pub node_id: String,
    pub kind: GateKind,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default)]
    pub condition: Value,
    pub metadata: Value,
}
