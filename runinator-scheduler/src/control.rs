// centralized helpers for state.control.* pointer access and mutation.
// every read defaults so legacy runs without new fields still behave.

use runinator_models::workflows::WorkflowRun;
use serde_json::{Map, Value};

pub fn pause_requested(workflow_run: &WorkflowRun) -> bool {
    workflow_run
        .state
        .pointer("/control/pause_requested")
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub fn ensure_control_object(state: &mut Value) -> &mut Map<String, Value> {
    if !state.is_object() {
        *state = Value::Object(Map::new());
    }
    let object = state.as_object_mut().expect("state is object");
    if !object.contains_key("control") {
        object.insert("control".into(), Value::Object(Map::new()));
    }
    object
        .get_mut("control")
        .and_then(Value::as_object_mut)
        .expect("control is object")
}
