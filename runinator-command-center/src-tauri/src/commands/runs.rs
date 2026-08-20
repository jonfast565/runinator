use super::*;

#[tauri::command]
pub async fn fetch_run_chunks(
    state: State<'_, CommandCenterState>,
    run_id: Uuid,
) -> CommandResult<Vec<RunChunk>> {
    get_json(&state, &format!("runs/{run_id}/chunks?limit=500")).await
}

#[tauri::command]
pub async fn fetch_run_artifacts(
    state: State<'_, CommandCenterState>,
    run_id: Uuid,
) -> CommandResult<Vec<RunArtifact>> {
    get_json(&state, &format!("runs/{run_id}/artifacts")).await
}

#[tauri::command]
pub async fn fetch_workflow_run_artifacts(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<WorkflowRunArtifact>> {
    get_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/artifacts"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_workflow_continuations(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<WorkflowContinuation>> {
    get_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/continuations"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_workflow_effects(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<WorkflowEffect>> {
    get_json(&state, &format!("workflow_runs/{workflow_run_id}/effects")).await
}

#[tauri::command]
pub async fn fetch_workflow_effect_output(
    state: State<'_, CommandCenterState>,
    effect_id: Uuid,
) -> CommandResult<Vec<WorkflowEffectOutputEvent>> {
    get_json(&state, &format!("workflow_effects/{effect_id}/output")).await
}

#[tauri::command]
pub async fn settle_workflow_effect(
    state: State<'_, CommandCenterState>,
    effect_id: Uuid,
    status: WorkflowEffectStatus,
    output: Option<Value>,
    message: Option<String>,
) -> CommandResult<TaskResponse> {
    let value = post_json(
        &state,
        &format!("workflow_effects/{effect_id}/settle"),
        &json!({ "status": status, "output": output, "message": message }),
    )
    .await?;
    serde_json::from_value(value).map_err(|error| CommandError::Unexpected(error.to_string()))
}

#[tauri::command]
pub async fn fetch_workflow_journal(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<WorkflowJournalRecord>> {
    get_json(&state, &format!("workflow_runs/{workflow_run_id}/journal")).await
}

#[tauri::command]
pub async fn fetch_workflow_vm_cursors(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<WorkflowVmCursor>> {
    get_json(&state, &format!("workflow_runs/{workflow_run_id}/cursors")).await
}

#[tauri::command]
pub async fn fetch_workflow_run_transitions(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<NodeTransition>> {
    get_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/transitions"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_workflow_node_transitions(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
    node_id: String,
) -> CommandResult<Vec<NodeTransitionStat>> {
    get_json(
        &state,
        &format!("workflows/{workflow_id}/nodes/{node_id}/transitions"),
    )
    .await
}

pub(super) async fn save_workflow_to_service(
    state: &CommandCenterState,
    workflow: &WorkflowDefinition,
) -> CommandResult<WorkflowDefinition> {
    let path = workflow
        .id
        .map(|id| format!("workflows/{id}"))
        .unwrap_or_else(|| "workflows".to_string());
    let url = build_state_url(state, &path).await?;
    let response = if workflow.id.is_some() {
        state
            .client
            .read()
            .await
            .patch(url.clone())
            .json(&workflow)
            .send()
            .await?
    } else {
        state
            .client
            .read()
            .await
            .post(url.clone())
            .json(&workflow)
            .send()
            .await?
    };
    let response = handle_response(url, response).await?;
    Ok(response.json::<WorkflowDefinition>().await?)
}

#[tauri::command]
pub async fn create_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
    debug: Option<bool>,
    parameters: Option<Value>,
) -> CommandResult<WorkflowRunCreated> {
    let url = build_state_url(&state, &format!("workflows/{workflow_id}/runs")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({
            "debug": debug.unwrap_or(false),
            "parameters": parameters.unwrap_or_else(|| json!({}))
        }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    let body = response.json::<Value>().await?;
    let id = body
        .get("run")
        .and_then(|run| run.get("id"))
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .ok_or_else(|| CommandError::Unexpected("missing workflow run id".into()))?;
    Ok(WorkflowRunCreated { id })
}

#[tauri::command]
pub async fn step_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    cursor: Option<Uuid>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/step"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "cursor": cursor }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn continue_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    cursor: Option<Uuid>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/continue"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "cursor": cursor }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

/// fork a speculative "what if" branch beside the run's real threads of control.
#[tauri::command]
pub async fn fork_workflow_run_cursor(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    from_cursor: Option<Uuid>,
    at_node: Option<String>,
    label: Option<String>,
    context_patch: Option<Value>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/fork"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({
            "from_cursor": from_cursor,
            "at_node": at_node,
            "label": label,
            "context_patch": context_patch,
        }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

/// send any `DebugVerb`. the verbs without a route of their own (retire_cursor, arm_for_real,
/// set_breakpoints, set_mode) ride this one, so the desktop path does not need a command each.
#[tauri::command]
pub async fn debug_command(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    verb: Value,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/command"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&verb)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn cancel_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/cancel")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn pause_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/pause")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn resume_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/resume")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn patch_workflow_run_debug(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    patch: Value,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/debug")).await?;
    let response = state
        .client
        .read()
        .await
        .patch(url.clone())
        .json(&patch)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn run_to_cursor_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    node_id: String,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/run_to_cursor"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "node_id": node_id }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn skip_workflow_node(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    output_json: Value,
    message: Option<String>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/skip"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "output_json": output_json, "message": message }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn rerun_workflow_node(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    parameters: Value,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/rerun_node"),
    )
    .await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "parameters": parameters }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn fetch_supervisor_status(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    let url = build_state_url(&state, "supervisor/status").await?;
    let response = state.client.read().await.get(url.clone()).send().await?;
    // accept both 200 (with snapshot) and 404 (configured: false) — both return JSON.
    if response.status().as_u16() == 404 {
        return Ok(response.json::<Value>().await?);
    }
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn replay_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    from_step_id: Option<String>,
) -> CommandResult<WorkflowRunCreated> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/replay")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "from_step_id": from_step_id }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    let body = response.json::<Value>().await?;
    let id = body
        .get("run")
        .and_then(|run| run.get("id"))
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
        .ok_or_else(|| CommandError::Unexpected("missing workflow run id".into()))?;
    Ok(WorkflowRunCreated { id })
}

#[tauri::command]
pub async fn rename_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    name: Option<String>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/rename")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "name": name }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn fetch_workflow_runs(
    state: State<'_, CommandCenterState>,
    workflow_id: Option<Uuid>,
) -> CommandResult<Vec<WorkflowRun>> {
    match workflow_id {
        Some(workflow_id) => {
            get_json(&state, &format!("workflow_runs?workflow_id={workflow_id}")).await
        }
        None => get_json(&state, "workflow_runs").await,
    }
}

#[tauri::command]
pub async fn fetch_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<WorkflowRunDetail> {
    let body: Value = get_json(&state, &format!("workflow_runs/{workflow_run_id}")).await?;
    let run = serde_json::from_value(
        body.get("run")
            .cloned()
            .ok_or_else(|| CommandError::Unexpected("missing workflow run".into()))?,
    )
    .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let nodes = serde_json::from_value(body.get("nodes").cloned().unwrap_or(Value::Array(vec![])))
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let continuations = get_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/continuations"),
    )
    .await?;
    let effects = get_json(&state, &format!("workflow_runs/{workflow_run_id}/effects")).await?;
    let journal = get_json(&state, &format!("workflow_runs/{workflow_run_id}/journal")).await?;
    let vm_cursors = get_json(&state, &format!("workflow_runs/{workflow_run_id}/cursors")).await?;
    Ok(WorkflowRunDetail {
        run,
        nodes,
        continuations,
        effects,
        journal,
        vm_cursors,
    })
}

#[tauri::command]
pub async fn delete_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<TaskResponse> {
    delete(&state, &format!("workflow_runs/{workflow_run_id}")).await
}
