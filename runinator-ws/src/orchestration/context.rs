use super::*;

pub(super) fn runtime_context(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Value {
    let prev_output = node_runs
        .iter()
        .filter_map(|run| run.output_json.clone())
        .next_back();
    let outputs = node_runs
        .iter()
        .filter_map(|run| {
            run.output_json
                .clone()
                .map(|output| (run.node_id.clone(), output))
        })
        .collect::<HashMap<_, _>>();
    let mut context = runinator_workflows::outputs_context(&workflow_run.parameters, &outputs);
    if let Some(object) = context.as_object_mut() {
        let header = WorkflowContextHeader {
            run_id: workflow_run.id,
            workflow_id: workflow_run.workflow_id,
            state: workflow_run.state.clone(),
        };
        object.insert(
            "workflow".into(),
            header.to_wire_value().unwrap_or(Value::Null),
        );
        if let Some(prev) = prev_output {
            object.insert("prev".into(), prev);
        }
        // config refs (`{"$ref":{"config":[...]}}`) resolve here, before any action command
        // is published; secrets stay unresolved until the worker.
        object.insert("config".into(), crate::handlers::credentials::config_tree());
    }
    // fill omitted input fields from their declared defaults, evaluated against this context (so a
    // default may read config/run/secret or a sibling input). resolved here, after config is in
    // place, so every downstream `input.*` ref sees the defaulted value.
    if let Some(snapshot) = &workflow_run.workflow_snapshot {
        runinator_workflows::apply_input_defaults(&mut context, &snapshot.input_type);
    }
    context
}

pub(super) fn set_step_output(scope: &mut Value, node_id: &str, output: Value) {
    if let Some(slot) = scope.pointer_mut(&format!("/steps/{node_id}/output")) {
        *slot = output;
    }
}

pub(super) fn merge_parameters(defaults: &Value, parameters: &Value) -> Value {
    match (defaults, parameters) {
        (Value::Object(defaults), Value::Object(parameters)) => {
            let mut merged = defaults.clone();
            for (key, value) in parameters {
                merged.insert(key.clone(), value.clone());
            }
            Value::Object(merged)
        }
        (_, Value::Null) => defaults.clone(),
        _ => parameters.clone(),
    }
}

pub(super) fn latest_node_run<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|run| run.node_id == node_id)
        .max_by_key(|run| run.id)
}

// true when a resumable node is re-entered with a terminal run from a prior visit. a loop body (or
// any back-edge) drives control past the node and returns to it, leaving the previous iteration's
// run as `latest`; the intervening control node always records a newer node run, so a node run
// created after `latest` means control already left and came back. such a node must start a fresh
// visit instead of resuming or transitioning from the stale run, otherwise the body only runs once.
pub(super) fn is_reentry_stale(latest: &WorkflowNodeRun, node_runs: &[WorkflowNodeRun]) -> bool {
    latest.status.is_terminal() && node_runs.iter().any(|run| run.id > latest.id)
}
