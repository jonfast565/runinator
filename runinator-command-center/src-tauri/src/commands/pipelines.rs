use super::*;

#[tauri::command]
pub async fn fetch_pipelines(state: State<'_, CommandCenterState>) -> CommandResult<Vec<Pipeline>> {
    get_json(&state, "pipelines").await
}

#[tauri::command]
pub async fn fetch_pipeline(
    state: State<'_, CommandCenterState>,
    pipeline_id: Uuid,
) -> CommandResult<Pipeline> {
    get_json(&state, &format!("pipelines/{pipeline_id}")).await
}

#[tauri::command]
pub async fn save_pipeline(
    state: State<'_, CommandCenterState>,
    pipeline: Pipeline,
) -> CommandResult<Pipeline> {
    let response = match pipeline.id {
        Some(id) => {
            let url = build_state_url(&state, &format!("pipelines/{id}")).await?;
            let response = state
                .client
                .read()
                .await
                .patch(url.clone())
                .json(&pipeline)
                .send()
                .await?;
            handle_response(url, response).await?
        }
        None => {
            let url = build_state_url(&state, "pipelines").await?;
            let response = state
                .client
                .read()
                .await
                .post(url.clone())
                .json(&pipeline)
                .send()
                .await?;
            handle_response(url, response).await?
        }
    };
    Ok(response.json::<Pipeline>().await?)
}

#[tauri::command]
pub async fn delete_pipeline(
    state: State<'_, CommandCenterState>,
    pipeline_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("pipelines/{pipeline_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn fetch_pipeline_triggers(
    state: State<'_, CommandCenterState>,
    pipeline_id: Uuid,
) -> CommandResult<Vec<PipelineTrigger>> {
    get_json(&state, &format!("pipelines/{pipeline_id}/triggers")).await
}

#[tauri::command]
pub async fn save_pipeline_trigger(
    state: State<'_, CommandCenterState>,
    trigger: PipelineTrigger,
    creating: bool,
) -> CommandResult<PipelineTrigger> {
    let path = if creating {
        format!("pipelines/{}/triggers", trigger.pipeline_id)
    } else {
        let id = trigger
            .id
            .ok_or_else(|| CommandError::Unexpected("missing pipeline trigger id".into()))?;
        format!("pipeline_triggers/{id}")
    };
    let url = build_state_url(&state, &path).await?;
    let response = if creating {
        state
            .client
            .read()
            .await
            .post(url.clone())
            .json(&trigger)
            .send()
            .await?
    } else {
        state
            .client
            .read()
            .await
            .patch(url.clone())
            .json(&trigger)
            .send()
            .await?
    };
    let response = handle_response(url, response).await?;
    Ok(response.json::<PipelineTrigger>().await?)
}

#[tauri::command]
pub async fn delete_pipeline_trigger(
    state: State<'_, CommandCenterState>,
    trigger_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("pipeline_triggers/{trigger_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn create_pipeline_run(
    state: State<'_, CommandCenterState>,
    pipeline_id: Uuid,
    parameters: Option<Value>,
) -> CommandResult<PipelineRun> {
    let url = build_state_url(&state, &format!("pipelines/{pipeline_id}/runs")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "parameters": parameters.unwrap_or_else(|| json!({})) }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<PipelineRun>().await?)
}

#[tauri::command]
pub async fn fetch_pipeline_runs(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<PipelineRun>> {
    get_json(&state, "pipeline_runs").await
}

#[tauri::command]
pub async fn fetch_pipeline_run(
    state: State<'_, CommandCenterState>,
    pipeline_run_id: Uuid,
) -> CommandResult<PipelineRunDetail> {
    get_json(&state, &format!("pipeline_runs/{pipeline_run_id}")).await
}

#[tauri::command]
pub async fn cancel_pipeline_run(
    state: State<'_, CommandCenterState>,
    pipeline_run_id: Uuid,
) -> CommandResult<TaskResponse> {
    let value = post_empty(&state, &format!("pipeline_runs/{pipeline_run_id}/cancel")).await?;
    serde_json::from_value(value)
        .map_err(|err| CommandError::Unexpected(format!("invalid cancel response: {err}")))
}

#[tauri::command]
pub async fn retry_pipeline_member(
    state: State<'_, CommandCenterState>,
    pipeline_run_id: Uuid,
    member_key: String,
    parameters: Option<Value>,
) -> CommandResult<PipelineMemberAttempt> {
    let mut url =
        build_state_url(&state, &format!("pipeline_runs/{pipeline_run_id}/members")).await?;
    url.path_segments_mut()
        .map_err(|_| CommandError::Unexpected("pipeline retry URL cannot be a base URL".into()))?
        .push(&member_key)
        .push("retry");
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({ "parameters": parameters.unwrap_or_else(|| json!({})) }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<PipelineMemberAttempt>().await?)
}

/// resolve a pipeline run's pending inquiry (a member with the `inquire` failure mode paused it).
#[tauri::command]
pub async fn resolve_pipeline_run(
    state: State<'_, CommandCenterState>,
    pipeline_run_id: Uuid,
    decision: String,
    resolved_by: Option<String>,
    message: Option<String>,
) -> CommandResult<PipelineRun> {
    let url = build_state_url(&state, &format!("pipeline_runs/{pipeline_run_id}/resolve")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({
            "decision": decision,
            "resolved_by": resolved_by,
            "message": message,
        }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<PipelineRun>().await?)
}
