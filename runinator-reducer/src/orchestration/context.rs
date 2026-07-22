use super::*;

pub(super) async fn runtime_context<T: DatabaseImpl>(
    db: &T,
    workflow_run: &WorkflowRun,
    node_runs: &[WorkflowNodeRun],
) -> Value {
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
        object.insert("config".into(), crate::config::config_tree(db).await);
    }
    // fill omitted input fields from their declared defaults, evaluated against this context (so a
    // default may read config/run/secret or a sibling input). resolved here, after config is in
    // place, so every downstream `input.*` ref sees the defaulted value.
    if let Some(snapshot) = &workflow_run.workflow_snapshot {
        runinator_workflows::apply_input_defaults(&mut context, &snapshot.input_type);
    }
    // expose each node's emitted artifacts under `steps.<node_id>.artifacts` so downstream nodes
    // (and output nodes declaring artifacts) can ref them like any other output value.
    inject_node_artifacts(db, workflow_run.id, node_runs, &mut context).await;
    context
}

// attach `steps.<node_id>.artifacts` for every node run that produced artifacts.
async fn inject_node_artifacts<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_runs: &[WorkflowNodeRun],
    context: &mut Value,
) {
    let artifacts = match db
        .fetch_workflow_node_run_artifacts_for_run(workflow_run_id)
        .await
    {
        Ok(artifacts) => artifacts,
        Err(_) => return,
    };
    if artifacts.is_empty() {
        return;
    }
    // map node-run id -> node id so artifacts land on the authored step, not the run uuid.
    let node_for_run: HashMap<Uuid, String> = node_runs
        .iter()
        .map(|run| (run.id, run.node_id.clone()))
        .collect();
    let mut by_node: HashMap<String, Vec<Value>> = HashMap::new();
    for artifact in artifacts {
        let Some(node_id) = node_for_run.get(&artifact.workflow_node_run_id) else {
            continue;
        };
        by_node
            .entry(node_id.clone())
            .or_default()
            .push(artifact_descriptor(&artifact));
    }
    for (node_id, list) in by_node {
        set_step_artifacts(context, &node_id, Value::Array(list));
    }
}

// the value-ref shape a workflow author sees for an artifact.
pub(super) fn artifact_descriptor(artifact: &WorkflowNodeRunArtifact) -> Value {
    runinator_models::json!({
        "id": artifact.id,
        "name": artifact.name,
        "mime_type": artifact.mime_type,
        "size_bytes": artifact.size_bytes,
        "uri": artifact.uri,
        "metadata": artifact.metadata,
    })
}

pub(super) fn set_step_output(scope: &mut Value, node_id: &str, output: Value) {
    if let Some(slot) = scope.pointer_mut(&format!("/steps/{node_id}/output")) {
        *slot = output;
    }
}

// set `steps.<node_id>.artifacts`, creating the step entry if the node produced artifacts but no
// `output_json` (so `outputs_context` never recorded a step for it).
pub(super) fn set_step_artifacts(scope: &mut Value, node_id: &str, artifacts: Value) {
    let Some(steps) = scope.pointer_mut("/steps").and_then(Value::as_object_mut) else {
        return;
    };
    let entry = steps
        .entry(node_id.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(object) = entry.as_object_mut() {
        object.insert("artifacts".into(), artifacts);
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

// the node run that most recently finished, used as the default origin for the next node run. in
// the single-cursor model the run this step transitioned from is the last one to settle, so its id
// is the correct `prev_node_run_id`. fan-out handlers override this with the explicit parent id.
pub(super) fn most_recently_finished_node_run(node_runs: &[WorkflowNodeRun]) -> Option<Uuid> {
    node_runs
        .iter()
        .filter(|run| run.finished_at.is_some())
        .max_by_key(|run| (run.finished_at, run.id))
        .map(|run| run.id)
}

// true when a resumable node is re-entered with a terminal run from a prior visit. a loop body (or
// any back-edge) drives control past the node and returns to it, leaving the previous iteration's
// run as `latest`; the intervening control node always records a newer node run, so a node run
// created after `latest` means control already left and came back. such a node must start a fresh
// visit instead of resuming or transitioning from the stale run, otherwise the body only runs once.
pub(super) fn is_reentry_stale(latest: &WorkflowNodeRun, node_runs: &[WorkflowNodeRun]) -> bool {
    latest.status.is_terminal() && node_runs.iter().any(|run| run.id > latest.id)
}
