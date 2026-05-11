use runinator_models::core::ScheduledTask;
use runinator_models::errors::SendableError;
use runinator_models::workflows::{WorkflowNode, WorkflowNodeRun, WorkflowRun};
use serde_json::Value;
use std::collections::HashMap;

pub fn latest_node_run<'a>(
    node_runs: &'a [WorkflowNodeRun],
    node_id: &str,
) -> Option<&'a WorkflowNodeRun> {
    node_runs
        .iter()
        .filter(|run| run.node_id == node_id)
        .max_by_key(|run| run.id)
}

pub fn build_node_parameters(
    task: &ScheduledTask,
    node: &WorkflowNode,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
) -> Result<Value, SendableError> {
    let base = merge_parameters(&task.default_parameters, &node.parameters);
    let context = runtime_context(workflow_run, node_runs);
    runinator_workflows::resolve_value_refs(&base, &context)
        .map_err(|err| -> SendableError { Box::new(err) })
}

pub fn runtime_context(workflow_run: &WorkflowRun, node_runs: &[WorkflowNodeRun]) -> Value {
    let prev_output = node_runs
        .iter()
        .filter_map(|run| run.output_json.clone())
        .last();
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
        object.insert(
            "workflow".into(),
            serde_json::json!({
                "run_id": workflow_run.id,
                "workflow_id": workflow_run.workflow_id,
                "state": workflow_run.state,
            }),
        );
        if let Some(prev) = prev_output {
            object.insert("prev".into(), prev);
        }
    }
    context
}

pub fn merge_parameters(defaults: &Value, parameters: &Value) -> Value {
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
