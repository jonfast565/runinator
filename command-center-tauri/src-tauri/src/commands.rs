use runinator_models::{
    providers::ProviderMetadata,
    runs::{RunArtifact, RunChunk},
    web::TaskResponse,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRunArtifact, WorkflowNodeRunChunk,
        WorkflowRun, WorkflowTrigger,
    },
};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::{
    client::{build_state_url, get_json, handle_response, post_empty},
    discovery::start_discovery_thread,
    error::{CommandError, CommandResult},
    state::CommandCenterState,
    types::{
        CredentialPutRequest, CredentialSummary, ServiceStatus, WorkflowRunCreated,
        WorkflowRunDetail,
    },
};

#[tauri::command]
pub async fn get_service_status(
    state: State<'_, CommandCenterState>,
) -> CommandResult<ServiceStatus> {
    Ok(ServiceStatus {
        service_url: state.service_url.read().await.clone(),
    })
}

#[tauri::command]
pub fn start_service_discovery(app: AppHandle, state: State<'_, CommandCenterState>) {
    start_discovery_thread(app, state.inner().clone());
}

#[tauri::command]
pub async fn fetch_workflows(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<WorkflowDefinition>> {
    get_json(&state, "workflows").await
}

#[tauri::command]
pub async fn save_workflow(
    state: State<'_, CommandCenterState>,
    workflow: WorkflowDefinition,
) -> CommandResult<WorkflowDefinition> {
    save_workflow_to_service(&state, &workflow).await
}

#[tauri::command]
pub async fn save_workflow_bundle(
    state: State<'_, CommandCenterState>,
    request: WorkflowBundle,
) -> CommandResult<WorkflowBundle> {
    let url = build_state_url(&state, "workflows/import").await?;
    let response = state.client.post(url.clone()).json(&request).send().await?;
    let response = handle_response(url, response).await?;
    let saved = response.json::<WorkflowBundle>().await?;
    let Some(workflow_id) = saved.workflows.first().and_then(|workflow| workflow.id) else {
        return Ok(saved);
    };
    get_json(&state, &format!("workflows/{workflow_id}/export")).await
}

#[tauri::command]
pub async fn delete_workflow(
    state: State<'_, CommandCenterState>,
    workflow_id: i64,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflows/{workflow_id}")).await?;
    let response = state.client.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn fetch_workflow_triggers(
    state: State<'_, CommandCenterState>,
    workflow_id: i64,
) -> CommandResult<Vec<WorkflowTrigger>> {
    get_json(&state, &format!("workflows/{workflow_id}/triggers")).await
}

#[tauri::command]
pub async fn save_workflow_trigger(
    state: State<'_, CommandCenterState>,
    trigger: WorkflowTrigger,
    creating: bool,
) -> CommandResult<WorkflowTrigger> {
    let path = if creating {
        format!("workflows/{}/triggers", trigger.workflow_id)
    } else {
        let id = trigger
            .id
            .ok_or_else(|| CommandError::Unexpected("missing workflow trigger id".into()))?;
        format!("workflow_triggers/{id}")
    };
    let url = build_state_url(&state, &path).await?;
    let response = if creating {
        state.client.post(url.clone()).json(&trigger).send().await?
    } else {
        state
            .client
            .patch(url.clone())
            .json(&trigger)
            .send()
            .await?
    };
    let response = handle_response(url, response).await?;
    Ok(response.json::<WorkflowTrigger>().await?)
}

#[tauri::command]
pub async fn delete_workflow_trigger(
    state: State<'_, CommandCenterState>,
    trigger_id: i64,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_triggers/{trigger_id}")).await?;
    let response = state.client.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn fetch_run_chunks(
    state: State<'_, CommandCenterState>,
    run_id: i64,
) -> CommandResult<Vec<RunChunk>> {
    get_json(&state, &format!("runs/{run_id}/chunks?limit=500")).await
}

#[tauri::command]
pub async fn fetch_run_artifacts(
    state: State<'_, CommandCenterState>,
    run_id: i64,
) -> CommandResult<Vec<RunArtifact>> {
    get_json(&state, &format!("runs/{run_id}/artifacts")).await
}

#[tauri::command]
pub async fn fetch_workflow_node_run_chunks(
    state: State<'_, CommandCenterState>,
    node_run_id: i64,
) -> CommandResult<Vec<WorkflowNodeRunChunk>> {
    get_json(
        &state,
        &format!("workflow_node_runs/{node_run_id}/chunks?limit=500"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_workflow_node_run_artifacts(
    state: State<'_, CommandCenterState>,
    node_run_id: i64,
) -> CommandResult<Vec<WorkflowNodeRunArtifact>> {
    get_json(
        &state,
        &format!("workflow_node_runs/{node_run_id}/artifacts"),
    )
    .await
}

async fn save_workflow_to_service(
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
            .patch(url.clone())
            .json(&workflow)
            .send()
            .await?
    } else {
        state
            .client
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
    workflow_id: i64,
    debug: Option<bool>,
) -> CommandResult<WorkflowRunCreated> {
    let url = build_state_url(&state, &format!("workflows/{workflow_id}/runs")).await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({ "debug": debug.unwrap_or(false) }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    let body = response.json::<Value>().await?;
    let id = body
        .get("run")
        .and_then(|run| run.get("id"))
        .and_then(Value::as_i64)
        .ok_or_else(|| CommandError::Unexpected("missing workflow run id".into()))?;
    Ok(WorkflowRunCreated { id })
}

#[tauri::command]
pub async fn step_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: i64,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/step"),
    )
    .await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn continue_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: i64,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/continue"),
    )
    .await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn cancel_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: i64,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/cancel")).await?;
    let response = state
        .client
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
    workflow_run_id: i64,
    patch: Value,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/debug")).await?;
    let response = state.client.patch(url.clone()).json(&patch).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn run_to_cursor_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: i64,
    node_id: String,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/run_to_cursor"),
    )
    .await?;
    let response = state
        .client
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
    workflow_run_id: i64,
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
    workflow_run_id: i64,
    parameters: Value,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(
        &state,
        &format!("workflow_runs/{workflow_run_id}/debug/rerun_node"),
    )
    .await?;
    let response = state
        .client
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
    let response = state.client.get(url.clone()).send().await?;
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
    workflow_run_id: i64,
    from_step_id: Option<String>,
) -> CommandResult<WorkflowRunCreated> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/replay")).await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({ "from_step_id": from_step_id }))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    let body = response.json::<Value>().await?;
    let id = body
        .get("run")
        .and_then(|run| run.get("id"))
        .and_then(Value::as_i64)
        .ok_or_else(|| CommandError::Unexpected("missing workflow run id".into()))?;
    Ok(WorkflowRunCreated { id })
}

#[tauri::command]
pub async fn rename_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: i64,
    name: Option<String>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_runs/{workflow_run_id}/rename")).await?;
    let response = state
        .client
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
    workflow_id: Option<i64>,
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
    workflow_run_id: i64,
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
    Ok(WorkflowRunDetail { run, nodes })
}

#[tauri::command]
pub async fn fetch_resource_records(
    state: State<'_, CommandCenterState>,
    endpoint: String,
) -> CommandResult<Vec<Value>> {
    get_json(&state, &endpoint).await
}

#[tauri::command]
pub async fn fetch_providers(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<ProviderMetadata>> {
    get_json(&state, "providers").await
}

#[tauri::command]
pub async fn fetch_credentials(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<CredentialSummary>> {
    get_json(&state, "credentials").await
}

#[tauri::command]
pub async fn save_credential(
    state: State<'_, CommandCenterState>,
    request: CredentialPutRequest,
) -> CommandResult<Value> {
    let url = build_state_url(&state, "credentials").await?;
    let response = state.client.post(url.clone()).json(&request).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_credential(
    state: State<'_, CommandCenterState>,
    scope: String,
    name: String,
) -> CommandResult<Value> {
    let mut url = build_state_url(&state, "credentials").await?;
    url.query_pairs_mut()
        .append_pair("scope", &scope)
        .append_pair("name", &name);
    let response = state.client.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn approve_approval(
    state: State<'_, CommandCenterState>,
    approval_id: i64,
) -> CommandResult<Value> {
    post_empty(&state, &format!("approvals/{approval_id}/approve")).await
}

#[tauri::command]
pub async fn reject_approval(
    state: State<'_, CommandCenterState>,
    approval_id: i64,
) -> CommandResult<Value> {
    post_empty(&state, &format!("approvals/{approval_id}/reject")).await
}

#[tauri::command]
pub async fn fetch_all_artifacts(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<RunArtifact>> {
    get_json(&state, "artifacts").await
}

#[derive(serde::Deserialize)]
pub struct ArtifactUploadRequest {
    pub run_id: i64,
    #[serde(default)]
    pub workflow_node_run_id: Option<i64>,
}

#[tauri::command]
pub async fn upload_artifact(
    state: State<'_, CommandCenterState>,
    app: AppHandle,
    request: ArtifactUploadRequest,
) -> CommandResult<RunArtifact> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog().file().pick_file(move |file_path| {
        let path = file_path.and_then(|fp| fp.into_path().ok());
        let _ = tx.send(path);
    });
    let path = rx
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?
        .ok_or_else(|| CommandError::Unexpected("upload canceled".into()))?;

    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("artifact")
        .to_string();
    let mime_type = mime_guess::from_path(&path)
        .first_or_octet_stream()
        .essence_str()
        .to_string();

    let url = build_state_url(&state, "artifacts/upload").await?;
    let mut form = reqwest::multipart::Form::new()
        .text("run_id", request.run_id.to_string())
        .text("name", file_name.clone())
        .text("mime_type", mime_type);
    if let Some(node_run_id) = request.workflow_node_run_id {
        form = form.text("workflow_node_run_id", node_run_id.to_string());
    }
    form = form.part(
        "file",
        reqwest::multipart::Part::bytes(bytes).file_name(file_name),
    );

    let response = state
        .client
        .post(url.clone())
        .multipart(form)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<RunArtifact>().await?)
}

#[derive(serde::Serialize)]
pub struct ArtifactDownloadResult {
    pub saved_to: Option<String>,
}

#[tauri::command]
pub async fn download_artifact(
    state: State<'_, CommandCenterState>,
    app: AppHandle,
    artifact_id: i64,
    default_name: String,
) -> CommandResult<ArtifactDownloadResult> {
    use tauri_plugin_dialog::DialogExt;
    use tokio::sync::oneshot;

    let (tx, rx) = oneshot::channel::<Option<std::path::PathBuf>>();
    app.dialog()
        .file()
        .set_file_name(&default_name)
        .save_file(move |file_path| {
            let path = file_path.and_then(|fp| fp.into_path().ok());
            let _ = tx.send(path);
        });
    let Some(target) = rx
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?
    else {
        return Ok(ArtifactDownloadResult { saved_to: None });
    };

    let url = build_state_url(&state, &format!("artifacts/{artifact_id}/download")).await?;
    let response = state.client.get(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    let bytes = response.bytes().await?;
    tokio::fs::write(&target, bytes)
        .await
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    Ok(ArtifactDownloadResult {
        saved_to: Some(target.to_string_lossy().into_owned()),
    })
}

#[tauri::command]
pub async fn fetch_notifications(
    state: State<'_, CommandCenterState>,
    unread_only: bool,
    limit: i64,
) -> CommandResult<Vec<Value>> {
    let mut path = format!("notifications?limit={limit}");
    if unread_only {
        path.push_str("&unread=true");
    }
    get_json(&state, &path).await
}

#[tauri::command]
pub async fn mark_notification_read(
    state: State<'_, CommandCenterState>,
    notification_id: i64,
) -> CommandResult<Value> {
    post_empty(
        &state,
        &format!("notifications/{notification_id}/mark_read"),
    )
    .await
}

#[tauri::command]
pub async fn mark_all_notifications_read(
    state: State<'_, CommandCenterState>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, "notifications/mark_all_read").await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}
