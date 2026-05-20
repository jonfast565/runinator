// centralized helpers for state.debug.* pointer access and mutation.
// every read defaults so legacy runs without new fields still behave.

use runinator_models::workflows::WorkflowRun;
use serde_json::{Map, Value};

pub const MODE_STEP_ALL: &str = "step_all";
pub const MODE_BREAKPOINTS: &str = "breakpoints";

pub fn enabled(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/debug/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn paused(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/debug/paused")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn step_requested(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/debug/step_requested")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn mode(workflow_run: &WorkflowRun) -> &'static str {
    match workflow_run
        .state
        .pointer("/debug/mode")
        .and_then(Value::as_str)
    {
        Some(MODE_BREAKPOINTS) => MODE_BREAKPOINTS,
        _ => MODE_STEP_ALL,
    }
}

pub fn breakpoints(workflow_run: &WorkflowRun) -> Vec<String> {
    workflow_run
        .state
        .pointer("/debug/breakpoints")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn one_shot_breakpoint(workflow_run: &WorkflowRun) -> Option<String> {
    workflow_run
        .state
        .pointer("/debug/one_shot_breakpoint")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
}

/// returns true when this node should pause the run based on mode and breakpoints.
pub fn should_break_at(workflow_run: &WorkflowRun, node_id: &str) -> bool {
    if mode(workflow_run) == MODE_STEP_ALL {
        return true;
    }
    if breakpoints(workflow_run).iter().any(|id| id == node_id) {
        return true;
    }
    matches!(one_shot_breakpoint(workflow_run), Some(id) if id == node_id)
}

/// after consuming a step or one-shot breakpoint, clear those flags but
/// preserve user-owned fields like mode and breakpoints.
pub fn state_with_step_cleared(mut state: Value) -> Value {
    if let Some(debug) = state.get_mut("debug").and_then(Value::as_object_mut) {
        debug.insert("paused".into(), Value::Bool(false));
        debug.insert("step_requested".into(), Value::Bool(false));
        debug.insert("one_shot_breakpoint".into(), Value::Null);
    }
    state
}

/// ensure the debug object exists and return a mutable reference to it.
pub fn ensure_debug_object(state: &mut Value) -> &mut Map<String, Value> {
    if !state.is_object() {
        *state = Value::Object(Map::new());
    }
    let object = state.as_object_mut().expect("state is object");
    if !object.contains_key("debug") {
        object.insert("debug".into(), Value::Object(Map::new()));
    }
    object
        .get_mut("debug")
        .and_then(Value::as_object_mut)
        .expect("debug is object")
}

/// merge a user-supplied debug patch into existing state.
/// supports: breakpoints, mode, one_shot_breakpoint. unknown keys are ignored.
pub fn merge_user_debug_patch(state: &mut Value, patch: &Value) {
    let Some(patch_obj) = patch.as_object() else {
        return;
    };
    let debug = ensure_debug_object(state);
    if let Some(bps) = patch_obj.get("breakpoints") {
        if bps.is_array() {
            debug.insert("breakpoints".into(), bps.clone());
        }
    }
    if let Some(m) = patch_obj.get("mode").and_then(Value::as_str) {
        if m == MODE_STEP_ALL || m == MODE_BREAKPOINTS {
            debug.insert("mode".into(), Value::String(m.to_string()));
        }
    }
    if let Some(osb) = patch_obj.get("one_shot_breakpoint") {
        if osb.is_null() {
            debug.insert("one_shot_breakpoint".into(), Value::Null);
        } else if let Some(s) = osb.as_str() {
            debug.insert(
                "one_shot_breakpoint".into(),
                Value::String(s.to_string()),
            );
        }
    }
}
