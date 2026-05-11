use runinator_models::{
    core::ScheduledTask,
    runs::{RunArtifact, RunChunk, RunSummary},
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowRun},
};
use serde_json::{json, Value};
use tauri::{AppHandle, State};

use crate::{
    client::{build_state_url, get_json, handle_response, post_empty},
    discovery::start_discovery_thread,
    error::{CommandError, CommandResult},
    state::CommandCenterState,
    types::{
        SaveTaskRequest, SaveTaskResponse, ServiceStatus, WorkflowRunCreated, WorkflowRunDetail,
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
pub async fn fetch_tasks(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<ScheduledTask>> {
    get_json(&state, "tasks").await
}

#[tauri::command]
pub async fn save_task(
    state: State<'_, CommandCenterState>,
    request: SaveTaskRequest,
) -> CommandResult<SaveTaskResponse> {
    let path = match (request.creating, request.task.id) {
        (true, _) => "tasks".to_string(),
        (false, Some(id)) => format!("tasks/{id}"),
        (false, None) => return Err(CommandError::Unexpected("Task is missing an ID".into())),
    };
    let url = build_state_url(&state, &path).await?;
    let response = if request.creating {
        state
            .client
            .post(url.clone())
            .json(&request.task)
            .send()
            .await?
    } else {
        state
            .client
            .patch(url.clone())
            .json(&request.task)
            .send()
            .await?
    };
    let response = handle_response(url, response).await?;
    let body = response.json::<TaskResponse>().await?;
    Ok(SaveTaskResponse {
        success: body.success,
        message: body.message,
        creating: request.creating,
    })
}

#[tauri::command]
pub async fn request_task_run(
    state: State<'_, CommandCenterState>,
    task_id: i64,
) -> CommandResult<Value> {
    post_empty(&state, &format!("tasks/{task_id}/request_run")).await
}

#[tauri::command]
pub async fn fetch_task_runs(
    state: State<'_, CommandCenterState>,
    task_id: i64,
) -> CommandResult<Vec<RunSummary>> {
    get_json(&state, &format!("tasks/{task_id}/runs")).await
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
    let path = workflow
        .id
        .map(|id| format!("workflows/{id}"))
        .unwrap_or_else(|| "workflows".to_string());
    let url = build_state_url(&state, &path).await?;
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
) -> CommandResult<WorkflowRunCreated> {
    let url = build_state_url(&state, &format!("workflows/{workflow_id}/runs")).await?;
    let response = state
        .client
        .post(url.clone())
        .json(&json!({}))
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
pub async fn fetch_workflow_runs(
    state: State<'_, CommandCenterState>,
    workflow_id: i64,
) -> CommandResult<Vec<WorkflowRun>> {
    get_json(&state, &format!("workflow_runs?workflow_id={workflow_id}")).await
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
