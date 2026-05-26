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
    let object = ensure_object(state);
    let control = object
        .entry("control")
        .or_insert_with(|| Value::Object(Map::new()));
    ensure_object(control)
}

fn ensure_object(value: &mut Value) -> &mut Map<String, Value> {
    loop {
        if let Value::Object(object) = value {
            return object;
        }
        *value = Value::Object(Map::new());
    }
}
