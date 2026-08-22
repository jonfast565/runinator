use super::support;
use super::*;
use uuid::Uuid;

pub async fn pause_workflow_run<T: WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let changed = db.pause_workflow_vm_run(workflow_run_id).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Paused {changed} workflow continuation(s)"),
    })
}

pub async fn resume_workflow_run<T: WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let changed = db.resume_workflow_vm_run(workflow_run_id, false).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Resumed {changed} workflow continuation(s)"),
    })
}

pub async fn cancel_workflow_run<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    broker: &dyn Broker,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let effect_ids = db
        .cancel_workflow_vm_run(workflow_run_id, "workflow run canceled".into())
        .await?;
    for effect_id in effect_ids {
        if let Err(err) = publish_worker_control_command(
            broker,
            ControlCommand::for_effect(workflow_run_id, effect_id, ControlKind::Cancel),
        )
        .await
        {
            log::warn!("Failed to publish VM effect cancel {effect_id}: {err}");
        }
    }
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} canceled"),
    })
}

async fn publish_worker_control_command(
    broker: &dyn Broker,
    command: ControlCommand,
) -> Result<(), SendableError> {
    broker
        .publish_control(command)
        .await
        .map_err(|err| crate::errors::CONTROL_PUBLISH.error(err))
}

/// Dispatch a VM-native debugger command against a run.
pub async fn apply_debug_command<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    verb: DebugVerb,
) -> Result<TaskResponse, SendableError> {
    match verb {
        DebugVerb::Step { cursor } => step_debug_cursor(db, workflow_run_id, cursor).await,
        DebugVerb::Continue { cursor } => continue_debug_cursor(db, workflow_run_id, cursor).await,
    }
}

/// advance one thread of control by exactly one node.
pub async fn step_debug_cursor<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    if let Some(continuation_id) = cursor {
        let Some(mut continuation) = db.fetch_workflow_continuation(continuation_id).await? else {
            return Err(crate::errors::RESUME_NOT_FOUND.error(continuation_id));
        };
        if continuation.workflow_run_id != workflow_run_id {
            return Err(crate::errors::RESUME_NOT_FOUND.error(continuation_id));
        }
        if continuation.status != runinator_models::workflow_vm::WorkflowContinuationStatus::Paused
        {
            return Err(crate::errors::RESUME_NOT_FOUND.error("continuation is not paused"));
        }
        continuation.status = runinator_models::workflow_vm::WorkflowContinuationStatus::Runnable;
        continuation.operator_paused = false;
        if let Some(debug) = continuation
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| match frame {
                runinator_models::workflow_vm::WorkflowFrame::Debug(debug) => Some(debug),
                _ => None,
            })
        {
            debug.paused = false;
            debug.step_requested = true;
        }
        db.commit_workflow_continuation(
            continuation.clone(),
            runinator_models::workflow_vm::WorkflowJournalEntry::Transitioned {
                continuation_id,
                instruction_pointer: continuation.instruction_pointer,
            },
        )
        .await?;
        return Ok(TaskResponse {
            success: true,
            message: format!("Continuation {continuation_id} stepped"),
        });
    }
    let changed = db.resume_workflow_vm_run(workflow_run_id, true).await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Stepped {changed} workflow continuation(s)"),
    })
}

/// resume one thread of control, still honoring breakpoints.
pub async fn continue_debug_cursor<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    cursor: Option<Uuid>,
) -> Result<TaskResponse, SendableError> {
    if cursor.is_none() {
        let changed = db.resume_workflow_vm_run(workflow_run_id, false).await?;
        return Ok(TaskResponse {
            success: true,
            message: format!("Resumed {changed} workflow continuation(s)"),
        });
    }
    let continuation_id = cursor.expect("checked above");
    let Some(mut continuation) = db.fetch_workflow_continuation(continuation_id).await? else {
        return Err(crate::errors::RESUME_NOT_FOUND.error(continuation_id));
    };
    if continuation.workflow_run_id != workflow_run_id {
        return Err(crate::errors::RESUME_NOT_FOUND.error(continuation_id));
    }
    continuation.status = runinator_models::workflow_vm::WorkflowContinuationStatus::Runnable;
    continuation.operator_paused = false;
    if let Some(debug) = continuation
        .frames
        .iter_mut()
        .rev()
        .find_map(|frame| match frame {
            runinator_models::workflow_vm::WorkflowFrame::Debug(debug) => Some(debug),
            _ => None,
        })
    {
        debug.paused = false;
        debug.step_requested = false;
    }
    db.commit_workflow_continuation(
        continuation.clone(),
        runinator_models::workflow_vm::WorkflowJournalEntry::Transitioned {
            continuation_id,
            instruction_pointer: continuation.instruction_pointer,
        },
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Continuation {continuation_id} resumed"),
    })
}

pub async fn replay_workflow_run<T: RuntimeStore + WorkflowVmStore>(
    db: &T,
    workflow_run_id: Uuid,
    from_step_id: Option<String>,
) -> Result<WorkflowRun, SendableError> {
    let Some(source) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::REPLAY_NOT_FOUND.error(workflow_run_id));
    };
    let snapshot = match source.workflow_snapshot.clone() {
        Some(snap) => snap,
        None => support::fetch_workflow_snapshot(db, source.workflow_id).await?,
    };

    let mut state = runinator_models::json!({
        "control": { "pause_requested": false },
        "debug": {
            "enabled": true,
            "paused": false,
            "step_requested": false,
            "mode": "breakpoints",
            "breakpoints": [],
            "one_shot_breakpoint": null
        },
        "replay": { "source_run_id": source.id }
    });

    // phase d: support resuming from a specific step.
    if let Some(target_node_id) = from_step_id.as_deref() {
        // Validate the target against the frozen graph. Replay now starts directly at the compiled
        // source-map boundary; it must never manufacture historical node-run rows.
        ancestors_in_snapshot(&snapshot, target_node_id)?;
        if let Some(replay) = state.get_mut("replay").and_then(Value::as_object_mut) {
            replay.insert(
                "from_step_id".to_string(),
                Value::String(target_node_id.into()),
            );
        }
        let new_run = super::runs::create_workflow_vm_run(
            db,
            source.workflow_id,
            snapshot.clone(),
            source.parameters.clone(),
            state,
            source.name.clone(),
            runinator_models::replicas::WorkflowRunProvenance {
                source_kind: Some(runinator_models::replicas::TriggerSourceKind::Replay),
                actor_type: Some(runinator_models::replicas::TriggerActorType::System),
                actor_replica_id: None,
                actor_display_name: Some("replay".into()),
                request_host: None,
                request_ip: None,
                metadata: runinator_models::json!({ "source_run_id": source.id }),
            },
            None,
            Some(target_node_id),
        )
        .await?;

        db.update_workflow_run_status(
            new_run.id,
            WorkflowStatus::Queued,
            Some(target_node_id.to_string()),
            None,
            Some(format!(
                "Replayed from run {} starting at step {}",
                source.id, target_node_id
            )),
        )
        .await?;
        let Some(refreshed) = db.fetch_workflow_run(new_run.id).await? else {
            return Err(crate::errors::REPLAY_NOT_FOUND
                .error(format!("replay run {} disappeared", new_run.id)));
        };
        return Ok(refreshed);
    }

    super::runs::create_workflow_vm_run(
        db,
        source.workflow_id,
        snapshot,
        source.parameters,
        state,
        source.name,
        runinator_models::replicas::WorkflowRunProvenance {
            source_kind: Some(runinator_models::replicas::TriggerSourceKind::Replay),
            actor_type: Some(runinator_models::replicas::TriggerActorType::System),
            actor_replica_id: None,
            actor_display_name: Some("replay".into()),
            request_host: None,
            request_ip: None,
            metadata: runinator_models::json!({ "source_run_id": source.id }),
        },
        None,
        None,
    )
    .await
}

/// BFS over reverse transitions from `target_node_id` to find all nodes that must
/// have completed before the target can run. Refuses to traverse through
/// `Loop`/`Map`/`Parallel`/`Try` ancestors — multi-iteration state can't be
/// safely copied in v1 (Phase D limitation).
pub fn ancestors_in_snapshot(
    snapshot: &WorkflowDefinition,
    target_node_id: &str,
) -> Result<Vec<String>, SendableError> {
    use runinator_models::workflows::{WorkflowNode, WorkflowNodeKind};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    let nodes: Vec<WorkflowNode> = snapshot.definition.nodes.clone();

    if nodes.is_empty() {
        return Ok(Vec::new());
    }

    if !nodes.iter().any(|node| node.id == target_node_id) {
        return Err(crate::errors::REPLAY_MISSING_STEP.error(target_node_id));
    }

    // build reverse adjacency: for each node, the set of nodes that transition into it.
    let mut reverse: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let by_id: BTreeMap<&str, &WorkflowNode> =
        nodes.iter().map(|node| (node.id.as_str(), node)).collect();
    for node in &nodes {
        for child_id in transition_targets(node) {
            reverse.entry(child_id).or_default().insert(node.id.clone());
        }
    }

    let mut visited: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    if let Some(parents) = reverse.get(target_node_id) {
        for parent in parents {
            queue.push_back(parent.clone());
        }
    }

    while let Some(node_id) = queue.pop_front() {
        if !visited.insert(node_id.clone()) {
            continue;
        }
        if let Some(node) = by_id.get(node_id.as_str())
            && matches!(
                node.kind,
                WorkflowNodeKind::Loop
                    | WorkflowNodeKind::Map
                    | WorkflowNodeKind::Parallel
                    | WorkflowNodeKind::Try
                    | WorkflowNodeKind::Race
            )
        {
            return Err(crate::errors::REPLAY_CONTROL_FLOW.error(format!(
                "cannot restart from step {target_node_id}: ancestor {node_id} is a control-flow node ({:?}) whose state is not safely replayable",
                node.kind
            )));
        }
        if let Some(parents) = reverse.get(&node_id) {
            for parent in parents {
                queue.push_back(parent.clone());
            }
        }
    }

    // topologically sort the ancestor set so each node only depends on earlier-seeded outputs.
    let mut order = Vec::new();
    let mut remaining: BTreeSet<String> = visited.clone();
    while !remaining.is_empty() {
        // pick any node in `remaining` whose ancestors are all already placed.
        let next = remaining
            .iter()
            .find(|node_id| {
                reverse
                    .get(*node_id)
                    .map(|parents| parents.iter().all(|parent| !remaining.contains(parent)))
                    .unwrap_or(true)
            })
            .cloned();
        if let Some(node_id) = next {
            remaining.remove(&node_id);
            order.push(node_id);
        } else {
            // fallback: cycle detected; fall back to insertion order.
            order.extend(remaining.iter().cloned());
            remaining.clear();
        }
    }
    Ok(order)
}

fn transition_targets(node: &runinator_models::workflows::WorkflowNode) -> Vec<String> {
    use runinator_models::value::Value;
    let mut targets = Vec::new();
    fn walk(value: &Value, into: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                if let Some(target) = map.get("$node").and_then(|value| value.as_str()) {
                    into.push(target.to_string());
                    return;
                }
                for value in map.values() {
                    walk(value, into);
                }
            }
            Value::Array(items) => {
                for value in items {
                    walk(value, into);
                }
            }
            _ => {}
        }
    }
    let transitions_value = serde_json::to_value(&node.transitions)
        .map(Value::from)
        .unwrap_or(Value::Null);
    walk(&transitions_value, &mut targets);
    let condition_value = node.condition.to_value();
    walk(&condition_value, &mut targets);
    walk(&node.parameters, &mut targets);
    targets
}
