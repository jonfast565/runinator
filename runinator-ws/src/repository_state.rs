use runinator_models::workflows::WorkflowNodeRun;
use serde_json::Value;

pub(crate) fn ensure_debug_object(state: &mut Value) -> &mut serde_json::Map<String, Value> {
    let object = ensure_object(state);
    let debug = object
        .entry("debug")
        .or_insert_with(|| Value::Object(Default::default()));
    ensure_object(debug)
}

pub(crate) fn ensure_control_object(state: &mut Value) -> &mut serde_json::Map<String, Value> {
    let object = ensure_object(state);
    let control = object
        .entry("control")
        .or_insert_with(|| Value::Object(Default::default()));
    ensure_object(control)
}

fn ensure_object(value: &mut Value) -> &mut serde_json::Map<String, Value> {
    loop {
        if let Value::Object(object) = value {
            return object;
        }
        *value = Value::Object(Default::default());
    }
}

pub(crate) fn latest_node_run_for<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|node_run| node_run.node_id == node_id)
        .max_by_key(|node_run| node_run.attempt)
}
