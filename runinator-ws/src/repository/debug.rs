use super::support;
use super::*;
use uuid::Uuid;

/// dispatch a single canonical [`DebugVerb`] against a run. every debug operation funnels through
/// here so the per-verb behavior lives in exactly one place.
pub async fn apply_debug_command<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    verb: DebugVerb,
) -> Result<TaskResponse, SendableError> {
    match verb {
        DebugVerb::Step => step_debug_workflow_run(db, workflow_run_id).await,
        DebugVerb::Continue => continue_debug_workflow_run(db, workflow_run_id).await,
        DebugVerb::RunToCursor { cursor } => {
            run_to_cursor_workflow_run(db, workflow_run_id, cursor).await
        }
        DebugVerb::Skip { output, message } => {
            skip_debug_workflow_node(db, workflow_run_id, output, message).await
        }
        DebugVerb::Rerun { parameters } => {
            rerun_debug_workflow_node(db, workflow_run_id, parameters).await
        }
        DebugVerb::SetBreakpoints { breakpoints } => {
            set_debug_breakpoints(db, workflow_run_id, breakpoints).await
        }
        DebugVerb::SetMode { mode } => set_debug_mode(db, workflow_run_id, mode).await,
    }
}

/// load the typed run state, apply `mutate` to the debug frame (always marking it enabled), and
/// persist with the given status/message. the single typed write path for debug bookkeeping.
async fn persist_debug_frame<T: DatabaseImpl>(
    db: &T,
    run: &WorkflowRun,
    status: WorkflowStatus,
    message: Option<String>,
    mutate: impl FnOnce(&mut DebugFrame),
) -> Result<(), SendableError> {
    let mut run_state = WorkflowRunState::from_state(&run.state);
    let frame = run_state.debug.get_or_insert_with(DebugFrame::default);
    frame.config.enabled = true;
    mutate(frame);
    db.update_workflow_run_status(
        run.id,
        status,
        run.active_node_id.clone(),
        Some(run_state.to_state()),
        message,
    )
    .await
}

pub async fn step_debug_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    persist_debug_frame(
        db,
        &run,
        WorkflowStatus::Running,
        Some("Debug step requested".into()),
        |frame| {
            frame.runtime.paused = false;
            frame.runtime.step_requested = true;
        },
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug step requested".into(),
    })
}

pub async fn continue_debug_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    persist_debug_frame(
        db,
        &run,
        WorkflowStatus::Running,
        Some("Debug continue requested".into()),
        |frame| {
            frame.runtime.paused = false;
            frame.runtime.step_requested = false;
        },
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug continue requested".into(),
    })
}

pub async fn set_debug_breakpoints<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    breakpoints: Vec<String>,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let status = run.status;
    persist_debug_frame(db, &run, status, None, |frame| {
        frame.config.breakpoints = breakpoints;
    })
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Breakpoints updated".into(),
    })
}

pub async fn set_debug_mode<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    mode: DebugMode,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let status = run.status;
    persist_debug_frame(db, &run, status, None, |frame| {
        frame.config.mode = Some(mode);
    })
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug mode updated".into(),
    })
}

pub async fn update_workflow_run_debug<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    patch: Value,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let invalid = |detail: &str| crate::errors::DEBUG_INVALID_PATCH.error(detail);
    let patch_obj = patch
        .as_object()
        .ok_or_else(|| invalid("Debug patch must be a JSON object"))?;

    // validate the whole patch before touching state.
    let breakpoints = match patch_obj.get("breakpoints") {
        Some(bps) => Some(
            bps.as_array()
                .ok_or_else(|| invalid("breakpoints must be an array of node ids"))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| invalid("breakpoints must be an array of node ids"))
                })
                .collect::<Result<Vec<_>, _>>()?,
        ),
        None => None,
    };
    let mode = match patch_obj.get("mode") {
        Some(m) => Some(
            serde_json::from_value::<DebugMode>(m.clone().into())
                .map_err(|_| invalid("mode must be 'step_all' or 'breakpoints'"))?,
        ),
        None => None,
    };
    let one_shot = match patch_obj.get("one_shot_breakpoint") {
        Some(Value::Null) => Some(None),
        Some(osb) => Some(Some(
            osb.as_str()
                .ok_or_else(|| invalid("one_shot_breakpoint must be a node id string or null"))?
                .to_string(),
        )),
        None => None,
    };

    let status = run.status;
    persist_debug_frame(db, &run, status, None, |frame| {
        if let Some(breakpoints) = breakpoints {
            frame.config.breakpoints = breakpoints;
        }
        if let Some(mode) = mode {
            frame.config.mode = Some(mode);
        }
        if let Some(one_shot) = one_shot {
            frame.runtime.one_shot_breakpoint = one_shot;
        }
    })
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug settings updated".into(),
    })
}

pub async fn pause_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let command = ControlCommand::new(workflow_run_id, ControlKind::Pause);
    pause_workflow_run_command(db, &command).await
}

async fn pause_workflow_run_command<T: DatabaseImpl>(
    db: &T,
    command: &ControlCommand,
) -> Result<TaskResponse, SendableError> {
    let workflow_run_id = command.workflow_run_id;
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::PAUSE_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut run_state = WorkflowRunState::from_state(&run.state);
    run_state
        .control
        .get_or_insert_with(ControlFrame::default)
        .pause_requested = true;

    let node_runs = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let has_running_node = run
        .active_node_id
        .as_deref()
        .and_then(|node_id| latest_node_run_for(&node_runs, node_id))
        .is_some_and(|node_run| node_run.status == WorkflowStatus::Running);
    let debug_enabled = run_state
        .debug
        .as_ref()
        .map(|debug| debug.config.enabled)
        .unwrap_or(false);
    let status = if has_running_node || debug_enabled {
        run.status
    } else {
        WorkflowStatus::Paused
    };

    db.update_workflow_run_status(
        workflow_run_id,
        status,
        run.active_node_id,
        Some(run_state.to_state()),
        Some("Workflow pause requested".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} pause requested"),
    })
}

pub async fn resume_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let command = ControlCommand::new(workflow_run_id, ControlKind::Resume);
    resume_workflow_run_command(db, &command).await
}

async fn resume_workflow_run_command<T: DatabaseImpl>(
    db: &T,
    command: &ControlCommand,
) -> Result<TaskResponse, SendableError> {
    let workflow_run_id = command.workflow_run_id;
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::RESUME_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut run_state = WorkflowRunState::from_state(&run.state);
    run_state
        .control
        .get_or_insert_with(ControlFrame::default)
        .pause_requested = false;
    let status = if matches!(
        run.status,
        WorkflowStatus::Paused | WorkflowStatus::DebugPaused
    ) {
        WorkflowStatus::Running
    } else {
        run.status
    };
    if run.status == WorkflowStatus::DebugPaused {
        let debug = run_state.debug.get_or_insert_with(DebugFrame::default);
        debug.runtime.paused = false;
        debug.runtime.step_requested = false;
    }

    db.update_workflow_run_status(
        workflow_run_id,
        status,
        run.active_node_id,
        Some(run_state.to_state()),
        Some("Workflow resume requested".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} resumed"),
    })
}

pub async fn cancel_workflow_run<T: DatabaseImpl>(
    db: &T,
    broker: &dyn Broker,
    workflow_run_id: Uuid,
) -> Result<TaskResponse, SendableError> {
    let command = ControlCommand::new(workflow_run_id, ControlKind::Cancel);
    let response = cancel_workflow_run_command(db, &command).await?;
    publish_worker_control_command(broker, command).await?;
    Ok(response)
}

async fn cancel_workflow_run_command<T: DatabaseImpl>(
    db: &T,
    command: &ControlCommand,
) -> Result<TaskResponse, SendableError> {
    let workflow_run_id = command.workflow_run_id;
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::CANCEL_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut run_state = WorkflowRunState::from_state(&run.state);
    // clear paused / step_requested so any in-flight scheduler tick sees the cancel.
    if let Some(debug) = run_state.debug.as_mut() {
        debug.runtime.paused = false;
        debug.runtime.step_requested = false;
    }
    run_state
        .control
        .get_or_insert_with(ControlFrame::default)
        .pause_requested = false;
    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Canceled,
        run.active_node_id,
        Some(run_state.to_state()),
        Some("Workflow run canceled".into()),
    )
    .await?;
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

pub async fn run_to_cursor_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    node_id: String,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    persist_debug_frame(
        db,
        &run,
        WorkflowStatus::Running,
        Some(format!("Run to cursor at {}", node_id)),
        |frame| {
            frame.runtime.paused = false;
            frame.runtime.step_requested = false;
            frame.runtime.one_shot_breakpoint = Some(node_id.clone());
        },
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Running to cursor {}", node_id),
    })
}

pub async fn skip_debug_workflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    output_json: Value,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let active_node_id = run
        .active_node_id
        .clone()
        .ok_or_else(|| crate::errors::DEBUG_NO_ACTIVE_NODE.error("no node to skip"))?;
    let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let latest_node_run = nodes
        .into_iter()
        .filter(|n| n.node_id == active_node_id)
        .max_by_key(|n| n.attempt);
    let node_run = match latest_node_run {
        Some(n) => n,
        None => {
            db.create_workflow_node_run(workflow_run_id, active_node_id.clone(), Value::Null)
                .await?
        }
    };
    let skip_message = message.clone().unwrap_or_else(|| "Skipped in debug".into());
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        Some(output_json),
        None,
        Some(DEBUG_SKIPPED.into()),
        Some(skip_message),
    )
    .await?;

    persist_debug_frame(
        db,
        &run,
        WorkflowStatus::Running,
        Some(format!("Skipped node {}", active_node_id)),
        |frame| {
            frame.runtime.paused = false;
            frame.runtime.step_requested = true;
        },
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Skipped node {}", active_node_id),
    })
}

pub async fn rerun_debug_workflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
    parameters: Value,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let active_node_id = run
        .active_node_id
        .clone()
        .ok_or_else(|| crate::errors::DEBUG_NO_ACTIVE_NODE.error("no node to re-run"))?;
    let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let latest_node_run = nodes
        .into_iter()
        .filter(|n| n.node_id == active_node_id)
        .max_by_key(|n| n.attempt);
    let next_attempt = latest_node_run.as_ref().map(|r| r.attempt + 1).unwrap_or(1);
    if let Some(prior) = latest_node_run {
        db.update_workflow_node_run(
            prior.id,
            WorkflowStatus::Failed,
            None,
            None,
            None,
            None,
            Some(DEBUG_SUPERSEDED.into()),
            Some("Superseded by debug re-run".into()),
        )
        .await?;
    }
    let new_run = db
        .create_workflow_node_run(workflow_run_id, active_node_id.clone(), parameters)
        .await?;
    db.update_workflow_node_run(
        new_run.id,
        WorkflowStatus::Queued,
        Some(next_attempt),
        None,
        None,
        None,
        Some(DEBUG_RERUN.into()),
        None,
    )
    .await?;

    persist_debug_frame(
        db,
        &run,
        WorkflowStatus::Running,
        Some(format!("Re-running node {}", active_node_id)),
        |frame| {
            frame.runtime.paused = false;
            frame.runtime.step_requested = true;
        },
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Re-running node {}", active_node_id),
    })
}

pub async fn replay_workflow_run<T: DatabaseImpl>(
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
        let ancestor_ids = ancestors_in_snapshot(&snapshot, target_node_id)?;
        if let Some(replay) = state.get_mut("replay").and_then(Value::as_object_mut) {
            replay.insert(
                "from_step_id".to_string(),
                Value::String(target_node_id.into()),
            );
        }
        let new_run = db
            .create_workflow_run(
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
            )
            .await?;

        if !ancestor_ids.is_empty() {
            let source_nodes = db.fetch_workflow_node_runs(source.id).await?;
            for node_id in &ancestor_ids {
                if let Some(source_node) = source_nodes
                    .iter()
                    .rev()
                    .find(|node| node.node_id == *node_id && node.status.is_terminal())
                {
                    let new_node = db
                        .create_workflow_node_run(
                            new_run.id,
                            node_id.clone(),
                            source_node.parameters.clone(),
                        )
                        .await?;
                    let attempt = if source_node.attempt > 0 {
                        Some(source_node.attempt)
                    } else {
                        Some(1)
                    };
                    db.update_workflow_node_run(
                        new_node.id,
                        source_node.status,
                        attempt,
                        None,
                        source_node.output_json.clone(),
                        Some(source_node.state.clone()),
                        Some("replayed_from_source".into()),
                        Some(format!("Replayed from run {} step {}", source.id, node_id)),
                    )
                    .await?;
                }
            }
        }
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
        support::enqueue_node_ready(
            db,
            new_run.id,
            target_node_id.to_string(),
            "workflow_run_replay",
            Utc::now(),
            runinator_models::json!({ "node_id": target_node_id }),
        )
        .await?;

        let Some(refreshed) = db.fetch_workflow_run(new_run.id).await? else {
            return Err(crate::errors::REPLAY_NOT_FOUND
                .error(format!("replay run {} disappeared", new_run.id)));
        };
        return Ok(refreshed);
    }

    let run = db
        .create_workflow_run(
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
        )
        .await?;
    support::enqueue_start_ready_node(db, &run).await?;
    Ok(run)
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
    walk(&node.condition, &mut targets);
    walk(&node.parameters, &mut targets);
    targets
}

async fn require_debug_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: Uuid,
) -> Result<WorkflowRun, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(crate::errors::DEBUG_NOT_FOUND.error(workflow_run_id));
    };
    if run.status.is_terminal() {
        return Err(crate::errors::DEBUG_TERMINAL.error(workflow_run_id));
    }
    if !run
        .state
        .pointer("/debug/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(crate::errors::DEBUG_DISABLED.error(workflow_run_id));
    }
    Ok(run)
}
