use chrono::Utc;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    errors::{RuntimeError, SendableError},
    runs::{NewRunArtifact, NewRunChunk, RunChunk},
    web::TaskResponse,
    workflows::{
        WorkflowDefinition, WorkflowNodeRun, WorkflowNodeRunArtifact, WorkflowNodeRunChunk,
        WorkflowRun, WorkflowStatus, WorkflowTrigger,
    },
};
use serde_json::Value;

#[cfg(test)]
pub(crate) fn merge_json_object(defaults: &Value, parameters: &Value) -> Value {
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

pub async fn fetch_run_chunks<T: DatabaseImpl>(
    db: &T,
    run_id: i64,
    cursor: Option<i64>,
    limit: i64,
) -> Result<Vec<RunChunk>, SendableError> {
    db.fetch_run_chunks(run_id, cursor, limit).await
}

pub async fn upsert_workflow<T: DatabaseImpl>(
    db: &T,
    workflow: &WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let workflow = runinator_workflows::normalize_workflow(workflow);
    runinator_workflows::validate_workflow(&workflow)
        .map_err(|err| -> SendableError { Box::new(err) })?;
    db.upsert_workflow(&workflow).await
}

pub async fn fetch_workflows<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowDefinition>, SendableError> {
    let workflows = db.fetch_workflows().await?;
    let mut normalized = Vec::with_capacity(workflows.len());
    for workflow in workflows {
        normalized.push(normalize_persisted_workflow(db, workflow).await?);
    }
    Ok(normalized)
}

pub async fn fetch_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<Option<WorkflowDefinition>, SendableError> {
    let Some(workflow) = db.fetch_workflow(workflow_id).await? else {
        return Ok(None);
    };
    Ok(Some(normalize_persisted_workflow(db, workflow).await?))
}

async fn normalize_persisted_workflow<T: DatabaseImpl>(
    db: &T,
    workflow: WorkflowDefinition,
) -> Result<WorkflowDefinition, SendableError> {
    let normalized = runinator_workflows::normalize_workflow(&workflow);
    if normalized.definition == workflow.definition {
        return Ok(workflow);
    }
    db.upsert_workflow(&normalized).await
}

pub async fn delete_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow(workflow_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow deleted".into(),
    })
}

pub async fn upsert_workflow_trigger<T: DatabaseImpl>(
    db: &T,
    trigger: &WorkflowTrigger,
) -> Result<WorkflowTrigger, SendableError> {
    db.upsert_workflow_trigger(trigger).await
}

pub async fn fetch_workflow_triggers<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<Vec<WorkflowTrigger>, SendableError> {
    db.fetch_workflow_triggers(workflow_id).await
}

pub async fn fetch_workflow_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: i64,
) -> Result<Option<WorkflowTrigger>, SendableError> {
    db.fetch_workflow_trigger(trigger_id).await
}

pub async fn fetch_due_workflow_triggers<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowTrigger>, SendableError> {
    db.fetch_due_workflow_triggers(Utc::now()).await
}

pub async fn delete_workflow_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: i64,
) -> Result<TaskResponse, SendableError> {
    db.delete_workflow_trigger(trigger_id).await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow trigger deleted".into(),
    })
}

pub async fn create_workflow_run_for_trigger<T: DatabaseImpl>(
    db: &T,
    trigger_id: i64,
    parameters: Value,
    debug: bool,
) -> Result<WorkflowRun, SendableError> {
    let Some(trigger) = db.fetch_workflow_trigger(trigger_id).await? else {
        return Err(Box::new(RuntimeError::new(
            "workflow_trigger.not_found".into(),
            format!("Workflow trigger {trigger_id} not found"),
        )));
    };
    let workflow_snapshot = fetch_workflow_snapshot(db, trigger.workflow_id).await?;
    let mut state = trigger_state(&trigger);
    if debug {
        let debug_state = serde_json::json!({
            "enabled": true,
            "paused": false,
            "step_requested": false
        });
        if let Some(object) = state.as_object_mut() {
            object.insert("debug".into(), debug_state);
        }
    }
    db.create_workflow_run(trigger.workflow_id, workflow_snapshot, parameters, state)
        .await
}

async fn fetch_workflow_snapshot<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<WorkflowDefinition, SendableError> {
    db.fetch_workflow(workflow_id).await?.ok_or_else(|| {
        Box::new(RuntimeError::new(
            "workflow.not_found".into(),
            format!("Workflow {workflow_id} not found"),
        )) as SendableError
    })
}

fn trigger_state(trigger: &WorkflowTrigger) -> Value {
    serde_json::json!({
        "trigger": {
            "id": trigger.id,
            "kind": trigger.kind,
            "metadata": trigger.metadata
        }
    })
}

pub async fn create_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
    parameters: Value,
    debug: bool,
) -> Result<WorkflowRun, SendableError> {
    let workflow_snapshot = fetch_workflow_snapshot(db, workflow_id).await?;
    let state = if debug {
        serde_json::json!({
            "debug": {
                "enabled": true,
                "paused": false,
                "step_requested": false,
                "mode": "breakpoints",
                "breakpoints": [],
                "one_shot_breakpoint": null
            }
        })
    } else {
        Value::Object(Default::default())
    };
    db.create_workflow_run(workflow_id, workflow_snapshot, parameters, state)
        .await
}

pub async fn fetch_workflow_runs_by_status<T: DatabaseImpl>(
    db: &T,
    status: WorkflowStatus,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_by_status(status).await
}

pub async fn fetch_recent_workflow_runs<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_recent_workflow_runs().await
}

pub async fn fetch_workflow_runs_for_workflow<T: DatabaseImpl>(
    db: &T,
    workflow_id: i64,
) -> Result<Vec<WorkflowRun>, SendableError> {
    db.fetch_workflow_runs_for_workflow(workflow_id).await
}

pub async fn update_workflow_run_status<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    status: WorkflowStatus,
    active_node_id: Option<String>,
    state: Option<Value>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_run_status(workflow_run_id, status, active_node_id, state, message)
        .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow run updated".into(),
    })
}

pub async fn step_debug_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<TaskResponse, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(Box::new(RuntimeError::new(
            "workflow.debug.not_found".into(),
            format!("Workflow run {workflow_run_id} not found"),
        )));
    };
    if run.status.is_terminal() {
        return Err(Box::new(RuntimeError::new(
            "workflow.debug.terminal".into(),
            format!("Workflow run {workflow_run_id} is terminal"),
        )));
    }
    if !run
        .state
        .pointer("/debug/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(Box::new(RuntimeError::new(
            "workflow.debug.disabled".into(),
            format!("Workflow run {workflow_run_id} is not a debug run"),
        )));
    }

    let mut state = run.state;
    let debug = ensure_debug_object(&mut state);
    debug.insert("enabled".into(), Value::Bool(true));
    debug.insert("paused".into(), Value::Bool(false));
    debug.insert("step_requested".into(), Value::Bool(true));

    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        run.active_node_id,
        Some(state),
        Some("Debug step requested".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug step requested".into(),
    })
}

pub async fn continue_debug_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let mut state = run.state;
    let debug = ensure_debug_object(&mut state);
    debug.insert("enabled".into(), Value::Bool(true));
    debug.insert("paused".into(), Value::Bool(false));
    debug.insert("step_requested".into(), Value::Bool(false));

    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        run.active_node_id,
        Some(state),
        Some("Debug continue requested".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug continue requested".into(),
    })
}

pub async fn update_workflow_run_debug<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    patch: Value,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let mut state = run.state;
    let debug = ensure_debug_object(&mut state);

    let patch_obj = match patch.as_object() {
        Some(obj) => obj,
        None => {
            return Err(Box::new(RuntimeError::new(
                "workflow.debug.invalid_patch".into(),
                "Debug patch must be a JSON object".into(),
            )));
        }
    };
    if let Some(bps) = patch_obj.get("breakpoints") {
        if !bps.is_array() {
            return Err(Box::new(RuntimeError::new(
                "workflow.debug.invalid_patch".into(),
                "breakpoints must be an array of node ids".into(),
            )));
        }
        debug.insert("breakpoints".into(), bps.clone());
    }
    if let Some(m) = patch_obj.get("mode") {
        let mode = m
            .as_str()
            .ok_or_else(|| -> SendableError {
                Box::new(RuntimeError::new(
                    "workflow.debug.invalid_patch".into(),
                    "mode must be a string".into(),
                ))
            })?;
        if mode != "step_all" && mode != "breakpoints" {
            return Err(Box::new(RuntimeError::new(
                "workflow.debug.invalid_patch".into(),
                format!("mode must be 'step_all' or 'breakpoints', got {mode}"),
            )));
        }
        debug.insert("mode".into(), Value::String(mode.to_string()));
    }
    if let Some(osb) = patch_obj.get("one_shot_breakpoint") {
        if osb.is_null() {
            debug.insert("one_shot_breakpoint".into(), Value::Null);
        } else {
            let id = osb.as_str().ok_or_else(|| -> SendableError {
                Box::new(RuntimeError::new(
                    "workflow.debug.invalid_patch".into(),
                    "one_shot_breakpoint must be a node id string or null".into(),
                ))
            })?;
            debug.insert(
                "one_shot_breakpoint".into(),
                Value::String(id.to_string()),
            );
        }
    }

    db.update_workflow_run_status(
        workflow_run_id,
        run.status,
        run.active_node_id,
        Some(state),
        None,
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Debug settings updated".into(),
    })
}

pub async fn cancel_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<TaskResponse, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(Box::new(RuntimeError::new(
            "workflow.cancel.not_found".into(),
            format!("Workflow run {workflow_run_id} not found"),
        )));
    };
    if run.status.is_terminal() {
        return Ok(TaskResponse {
            success: true,
            message: format!("Workflow run {workflow_run_id} is already terminal"),
        });
    }
    let mut state = run.state.clone();
    if state.is_object() {
        // clear paused / step_requested so any in-flight scheduler tick sees the cancel.
        if let Some(obj) = state.as_object_mut() {
            if let Some(debug) = obj.get_mut("debug").and_then(Value::as_object_mut) {
                debug.insert("paused".into(), Value::Bool(false));
                debug.insert("step_requested".into(), Value::Bool(false));
            }
        }
    }
    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Canceled,
        run.active_node_id,
        Some(state),
        Some("Workflow run canceled".into()),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Workflow run {workflow_run_id} canceled"),
    })
}

pub async fn run_to_cursor_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    node_id: String,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let mut state = run.state;
    let debug = ensure_debug_object(&mut state);
    debug.insert("enabled".into(), Value::Bool(true));
    debug.insert("paused".into(), Value::Bool(false));
    debug.insert("step_requested".into(), Value::Bool(false));
    debug.insert(
        "one_shot_breakpoint".into(),
        Value::String(node_id.clone()),
    );

    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        run.active_node_id,
        Some(state),
        Some(format!("Run to cursor at {}", node_id)),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Running to cursor {}", node_id),
    })
}

pub async fn skip_debug_workflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    output_json: Value,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let active_node_id = run.active_node_id.clone().ok_or_else(|| -> SendableError {
        Box::new(RuntimeError::new(
            "workflow.debug.no_active_node".into(),
            "No active node to skip".into(),
        ))
    })?;
    let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let latest_node_run = nodes
        .into_iter()
        .filter(|n| n.node_id == active_node_id)
        .max_by_key(|n| n.attempt);
    let node_run = match latest_node_run {
        Some(n) => n,
        None => db
            .create_workflow_node_run(workflow_run_id, active_node_id.clone(), Value::Null)
            .await?,
    };
    let skip_message = message.clone().unwrap_or_else(|| "Skipped in debug".into());
    db.update_workflow_node_run(
        node_run.id,
        WorkflowStatus::Succeeded,
        None,
        None,
        Some(output_json),
        None,
        Some("debug_skipped".into()),
        Some(skip_message),
    )
    .await?;

    let mut state = run.state;
    let debug = ensure_debug_object(&mut state);
    debug.insert("enabled".into(), Value::Bool(true));
    debug.insert("paused".into(), Value::Bool(false));
    debug.insert("step_requested".into(), Value::Bool(true));

    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        run.active_node_id,
        Some(state),
        Some(format!("Skipped node {}", active_node_id)),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Skipped node {}", active_node_id),
    })
}

pub async fn rerun_debug_workflow_node<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    parameters: Value,
) -> Result<TaskResponse, SendableError> {
    let run = require_debug_run(db, workflow_run_id).await?;
    let active_node_id = run.active_node_id.clone().ok_or_else(|| -> SendableError {
        Box::new(RuntimeError::new(
            "workflow.debug.no_active_node".into(),
            "No active node to re-run".into(),
        ))
    })?;
    let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
    let latest_node_run = nodes
        .into_iter()
        .filter(|n| n.node_id == active_node_id)
        .max_by_key(|n| n.attempt);
    let next_attempt = latest_node_run
        .as_ref()
        .map(|r| r.attempt + 1)
        .unwrap_or(1);
    if let Some(prior) = latest_node_run {
        db.update_workflow_node_run(
            prior.id,
            WorkflowStatus::Failed,
            None,
            None,
            None,
            None,
            Some("debug_superseded".into()),
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
        Some("debug_rerun".into()),
        None,
    )
    .await?;

    let mut state = run.state;
    let debug = ensure_debug_object(&mut state);
    debug.insert("enabled".into(), Value::Bool(true));
    debug.insert("paused".into(), Value::Bool(false));
    debug.insert("step_requested".into(), Value::Bool(true));

    db.update_workflow_run_status(
        workflow_run_id,
        WorkflowStatus::Running,
        run.active_node_id,
        Some(state),
        Some(format!("Re-running node {}", active_node_id)),
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: format!("Re-running node {}", active_node_id),
    })
}

pub async fn replay_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<WorkflowRun, SendableError> {
    let Some(source) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(Box::new(RuntimeError::new(
            "workflow.replay.not_found".into(),
            format!("Workflow run {workflow_run_id} not found"),
        )));
    };
    let snapshot = match source.workflow_snapshot.clone() {
        Some(snap) => snap,
        None => fetch_workflow_snapshot(db, source.workflow_id).await?,
    };
    let state = serde_json::json!({
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
    db.create_workflow_run(source.workflow_id, snapshot, source.parameters, state)
        .await
}

async fn require_debug_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<WorkflowRun, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Err(Box::new(RuntimeError::new(
            "workflow.debug.not_found".into(),
            format!("Workflow run {workflow_run_id} not found"),
        )));
    };
    if run.status.is_terminal() {
        return Err(Box::new(RuntimeError::new(
            "workflow.debug.terminal".into(),
            format!("Workflow run {workflow_run_id} is terminal"),
        )));
    }
    if !run
        .state
        .pointer("/debug/enabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err(Box::new(RuntimeError::new(
            "workflow.debug.disabled".into(),
            format!("Workflow run {workflow_run_id} is not a debug run"),
        )));
    }
    Ok(run)
}

pub async fn fetch_workflow_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
) -> Result<Option<(WorkflowRun, Vec<WorkflowNodeRun>)>, SendableError> {
    let Some(run) = db.fetch_workflow_run(workflow_run_id).await? else {
        return Ok(None);
    };
    let nodes = db.fetch_workflow_node_runs(workflow_run_id).await?;
    Ok(Some((run, nodes)))
}

pub async fn fetch_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: i64,
) -> Result<Option<WorkflowNodeRun>, SendableError> {
    db.fetch_workflow_node_run(workflow_node_run_id).await
}

pub async fn append_workflow_node_run_chunk<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: i64,
    chunk: &NewRunChunk,
) -> Result<WorkflowNodeRunChunk, SendableError> {
    db.append_workflow_node_run_chunk(workflow_node_run_id, chunk)
        .await
}

pub async fn fetch_workflow_node_run_chunks<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: i64,
    cursor: Option<i64>,
    limit: i64,
) -> Result<Vec<WorkflowNodeRunChunk>, SendableError> {
    db.fetch_workflow_node_run_chunks(workflow_node_run_id, cursor, limit)
        .await
}

pub async fn add_workflow_node_run_artifact<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: i64,
    artifact: &NewRunArtifact,
) -> Result<WorkflowNodeRunArtifact, SendableError> {
    db.add_workflow_node_run_artifact(workflow_node_run_id, artifact)
        .await
}

pub async fn fetch_workflow_node_run_artifacts<T: DatabaseImpl>(
    db: &T,
    workflow_node_run_id: i64,
) -> Result<Vec<WorkflowNodeRunArtifact>, SendableError> {
    db.fetch_workflow_node_run_artifacts(workflow_node_run_id)
        .await
}

fn ensure_debug_object(state: &mut Value) -> &mut serde_json::Map<String, Value> {
    if !state.is_object() {
        *state = serde_json::json!({});
    }
    let object = state.as_object_mut().expect("state object");
    let debug = object
        .entry("debug")
        .or_insert_with(|| serde_json::json!({}));
    if !debug.is_object() {
        *debug = serde_json::json!({});
    }
    debug.as_object_mut().expect("debug object")
}

pub async fn create_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    workflow_run_id: i64,
    node_id: String,
    parameters: Value,
) -> Result<WorkflowNodeRun, SendableError> {
    db.create_workflow_node_run(workflow_run_id, node_id, parameters)
        .await
}

pub async fn update_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    node_run_id: i64,
    status: WorkflowStatus,
    attempt: Option<i64>,
    parameters: Option<Value>,
    output_json: Option<Value>,
    state: Option<Value>,
    transition_reason: Option<String>,
    message: Option<String>,
) -> Result<TaskResponse, SendableError> {
    db.update_workflow_node_run(
        node_run_id,
        status,
        attempt,
        parameters,
        output_json,
        state,
        transition_reason,
        message,
    )
    .await?;
    Ok(TaskResponse {
        success: true,
        message: "Workflow node run updated".into(),
    })
}

pub async fn upsert_catalog_item<T: DatabaseImpl>(
    db: &T,
    item: Value,
) -> Result<Value, SendableError> {
    db.upsert_catalog_item(item).await
}

pub async fn fetch_catalog_items<T: DatabaseImpl>(
    db: &T,
    item_type: Option<String>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_catalog_items(item_type).await
}

pub async fn fetch_catalog_item<T: DatabaseImpl>(
    db: &T,
    uri: String,
) -> Result<Option<Value>, SendableError> {
    db.fetch_catalog_item(uri).await
}

pub async fn create_automation_record<T: DatabaseImpl>(
    db: &T,
    record_type: &str,
    record: Value,
) -> Result<Value, SendableError> {
    db.create_automation_record(record_type.into(), record)
        .await
}

pub async fn fetch_automation_records<T: DatabaseImpl>(
    db: &T,
    record_type: &str,
    workflow_run_id: Option<i64>,
    external_item_id: Option<i64>,
) -> Result<Vec<Value>, SendableError> {
    db.fetch_automation_records(record_type.into(), workflow_run_id, external_item_id)
        .await
}

pub async fn put_idempotency_key<T: DatabaseImpl>(
    db: &T,
    scope: String,
    key: String,
    result: Value,
) -> Result<Value, SendableError> {
    db.put_idempotency_key(scope, key, result).await
}

pub async fn fetch_idempotency_key<T: DatabaseImpl>(
    db: &T,
    scope: String,
    key: String,
) -> Result<Option<Value>, SendableError> {
    db.fetch_idempotency_key(scope, key).await
}

pub async fn resolve_approval<T: DatabaseImpl>(
    db: &T,
    approval_id: i64,
    approved: bool,
    resolved_by: Option<String>,
    message: Option<String>,
    output_json: Option<Value>,
) -> Result<Value, SendableError> {
    let Some(mut approval) = db
        .fetch_automation_record("approval_requests".into(), approval_id)
        .await?
    else {
        return Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("Approval request {approval_id} not found"),
        )));
    };
    let now = Utc::now().timestamp();
    if let Some(object) = approval.as_object_mut() {
        object.insert(
            "status".into(),
            if approved { "approved" } else { "rejected" }.into(),
        );
        object.insert("resolved_at".into(), now.into());
        if let Some(resolved_by) = resolved_by {
            object.insert("resolved_by".into(), resolved_by.into());
        }
        if let Some(message) = &message {
            object.insert("message".into(), message.clone().into());
        }
    }
    let updated = db
        .update_automation_record("approval_requests".into(), approval_id, approval.clone())
        .await?;

    if let (Some(workflow_run_id), Some(node_id)) = (
        approval.get("workflow_run_id").and_then(Value::as_i64),
        approval.get("node_id").and_then(Value::as_str),
    ) {
        let node_runs = db.fetch_workflow_node_runs(workflow_run_id).await?;
        if let Some(node_run) = node_runs
            .iter()
            .filter(|run| run.node_id == node_id)
            .max_by_key(|run| run.id)
        {
            db.update_workflow_node_run(
                node_run.id,
                if approved {
                    WorkflowStatus::Succeeded
                } else {
                    WorkflowStatus::Blocked
                },
                None,
                None,
                Some(output_json.unwrap_or_else(|| {
                    serde_json::json!({
                        "approval_id": approval_id,
                        "approved": approved
                    })
                })),
                Some(serde_json::json!({
                    "approval_id": approval_id,
                    "approved": approved
                })),
                Some(if approved {
                    "approval_approved".into()
                } else {
                    "approval_rejected".into()
                }),
                message,
            )
            .await?;
        }
        db.update_workflow_run_status(
            workflow_run_id,
            if approved {
                WorkflowStatus::Running
            } else {
                WorkflowStatus::Blocked
            },
            Some(node_id.to_string()),
            None,
            None,
        )
        .await?;
    }

    Ok(updated)
}
