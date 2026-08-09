use super::*;

/// the node runs one thread of control may read.
///
/// a real cursor never sees a speculative branch's output, or a "what if" fork could change what the
/// run means — including making a join think a branch arrived. a speculative cursor sees its own
/// subtree layered over the real run, which is what makes the fork explore from where it forked.
///
/// an interrupt handler's work is invisible to the thread it suspended, and vice versa is not: the
/// handler reads the run's context normally, but nothing it does leaks into `steps.*` for the
/// resumed thread. a handler is a side-channel, so the only way it may influence the thread it
/// interrupted is the decision its `resume` node carries.
///
/// membership is keyed on the *node*, not the cursor, and that is the whole point: a handler cursor
/// retires the moment its region ends, so a cursor-keyed test would let the region's node runs
/// become visible again the instant control returned. region membership is a static property of the
/// graph and outlives the cursor, exactly as `WorkflowNodeRun::speculative` is persisted rather than
/// inferred for the same reason.
///
/// filtering once, at the dispatch site, is what isolates all ~35 handlers (and the join's
/// satisfaction check with them) without any of them having to know the concept exists.
pub(super) fn visible_node_runs(
    cursor: &RunCursor,
    state: &WorkflowRunState,
    node_runs: &[WorkflowNodeRun],
    interrupt_region_nodes: &std::collections::HashSet<String>,
) -> Vec<WorkflowNodeRun> {
    // a handler sees the region's work; every other thread sees none of it. regions are disjoint
    // from each other and only one interrupt is live at a time, so "am i a handler" is a complete
    // answer without naming which region.
    let in_region = |run: &WorkflowNodeRun| interrupt_region_nodes.contains(run.node_id.as_str());
    let region_visible = cursor.is_interrupt_handler();

    if !cursor.is_speculative() {
        return node_runs
            .iter()
            .filter(|run| !run.speculative)
            .filter(|run| region_visible || !in_region(run))
            .cloned()
            .collect();
    }
    // this fork's own lineage: itself and the forks it descends from. *not* its descendants —
    // those are divergent explorations forked off later, and letting a branch read them would show
    // it work that happened on a path it did not take.
    let visible = state.speculative_ancestry(cursor.id);
    node_runs
        .iter()
        .filter(|run| !run.speculative || run.cursor_id.is_some_and(|id| visible.contains(&id)))
        .filter(|run| region_visible || !in_region(run))
        .cloned()
        .collect()
}

/// recursively overlay `patch` onto `target`, object by object. unlike [`merge_parameters`], which
/// is one level deep, this reaches nested paths so a fork can patch `steps.<node>.output.status`
/// without replacing the whole step.
pub(super) fn deep_merge(target: &mut Value, patch: &Value) {
    let (Some(target_object), Some(patch_object)) = (target.as_object_mut(), patch.as_object())
    else {
        *target = patch.clone();
        return;
    };
    for (key, value) in patch_object {
        match target_object.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => {
                deep_merge(existing, value)
            }
            _ => {
                target_object.insert(key.clone(), value.clone());
            }
        }
    }
}

pub(super) async fn runtime_context<T: ReducerStore>(
    db: &T,
    workflow_run: &WorkflowRun,
    cursor: &RunCursor,
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
    // a speculative fork's "what if": overlaid last so it wins over everything resolved above. a
    // patch rather than a synthetic node run, because this way it reaches `input.*`, `config.*`, and
    // `workflow.state` too, not just step outputs.
    if let Some(frame) = &cursor.speculative
        && !frame.context_patch.is_null()
    {
        deep_merge(&mut context, &frame.context_patch);
    }
    // an interrupt handler region reads what raised it under `interrupt.*`. only a handler cursor
    // carries the frame, so the root simply does not exist for ordinary threads of control.
    if let Some(frame) = &cursor.interrupt
        && let Some(root) = context.as_object_mut()
    {
        root.insert(
            "interrupt".to_string(),
            runinator_models::json!({
                "source": frame.source.as_str(),
                "payload": frame.payload.clone(),
                "raised_at": frame.raised_at.to_rfc3339(),
                "node_id": frame.resume.node_id.clone(),
            }),
        );
    }
    context
}

// attach `steps.<node_id>.artifacts` for every node run that produced artifacts.
async fn inject_node_artifacts<T: ReducerStore>(
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

/// coerce a resolved value into a flat correlation/routing string: null or empty -> `None`, strings
/// trimmed, other scalars stringified. shared by the await-workflow key and correlation stamping.
pub(super) fn coerce_scalar_string(value: &Value) -> Option<String> {
    match value {
        Value::Null => None,
        Value::String(text) => {
            let text = text.trim();
            (!text.is_empty()).then(|| text.to_string())
        }
        other => Some(other.to_string()),
    }
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
//
// "control left and came back" is a question about one thread of control, so only this cursor's own
// newer runs count. a sibling branch recording a node run is not this branch re-entering, and
// counting it would restart a node that never actually looped. runs from before node runs carried a
// cursor are counted, so a run mid-upgrade keeps its previous behavior.
pub(super) fn is_reentry_stale(
    latest: &WorkflowNodeRun,
    node_runs: &[WorkflowNodeRun],
    cursor: &RunCursor,
) -> bool {
    latest.status.is_terminal()
        && node_runs
            .iter()
            .any(|run| run.id > latest.id && run.cursor_id.is_none_or(|id| id == cursor.id))
}
