use std::collections::HashMap;

use runinator_models::{
    core::ScheduledTask,
    providers::ProviderMetadata,
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
        CredentialPutRequest, CredentialSummary, SaveTaskRequest, SaveTaskResponse, ServiceStatus,
        WorkflowBundleSaveRequest, WorkflowBundleSaveResponse, WorkflowRunCreated,
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
    let body = save_task_to_service(&state, &request.task, request.creating).await?;
    Ok(SaveTaskResponse {
        success: body.success,
        message: body.message,
        creating: request.creating,
    })
}

#[tauri::command]
pub async fn delete_task(
    state: State<'_, CommandCenterState>,
    task_id: i64,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("tasks/{task_id}")).await?;
    let response = state.client.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn save_workflow_bundle(
    state: State<'_, CommandCenterState>,
    mut request: WorkflowBundleSaveRequest,
) -> CommandResult<WorkflowBundleSaveResponse> {
    let existing_tasks: Vec<ScheduledTask> = get_json(&state, "tasks").await?;
    let mut next_task_id = existing_tasks
        .iter()
        .filter_map(|task| task.id)
        .max()
        .unwrap_or(0)
        + 1;
    let mut task_id_map = HashMap::new();
    let mut saved_tasks = Vec::new();

    for draft in &mut request.tasks {
        let is_new = draft.task.id.unwrap_or(0) <= 0;
        if is_new {
            draft.task.id = Some(next_task_id);
            next_task_id += 1;
        }
        stamp_workflow_task_metadata(&mut draft.task, None, &draft.node_id);
        let response = save_task_to_service(&state, &draft.task, is_new).await?;
        if !response.success {
            return Err(CommandError::Unexpected(response.message));
        }
        let id = draft.task.id.ok_or_else(|| {
            CommandError::Unexpected("saved workflow task is missing an ID".into())
        })?;
        task_id_map.insert(draft.node_id.clone(), id);
        saved_tasks.push(draft.task.clone());
    }

    rewrite_workflow_task_ids(&mut request.workflow.definition, &task_id_map);
    let saved_workflow = save_workflow_to_service(&state, &request.workflow).await?;
    if let Some(workflow_id) = saved_workflow.id {
        for task in &mut saved_tasks {
            let node_id = task
                .metadata
                .get("workflow_node_id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_default();
            stamp_workflow_task_metadata(task, Some(workflow_id), &node_id);
            let response = save_task_to_service(&state, task, false).await?;
            if !response.success {
                return Err(CommandError::Unexpected(response.message));
            }
        }
    }

    Ok(WorkflowBundleSaveResponse {
        workflow: saved_workflow,
        task_id_map,
        tasks: saved_tasks,
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
    save_workflow_to_service(&state, &workflow).await
}

async fn save_task_to_service(
    state: &CommandCenterState,
    task: &ScheduledTask,
    creating: bool,
) -> CommandResult<TaskResponse> {
    let path = match (creating, task.id) {
        (true, _) => "tasks".to_string(),
        (false, Some(id)) => format!("tasks/{id}"),
        (false, None) => return Err(CommandError::Unexpected("Task is missing an ID".into())),
    };
    let url = build_state_url(state, &path).await?;
    let response = if creating {
        state.client.post(url.clone()).json(task).send().await?
    } else {
        state.client.patch(url.clone()).json(task).send().await?
    };
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
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

fn rewrite_workflow_task_ids(definition: &mut Value, task_id_map: &HashMap<String, i64>) {
    let Some(nodes) = definition.get_mut("nodes").and_then(Value::as_array_mut) else {
        return;
    };
    for node in nodes {
        let Some(object) = node.as_object_mut() else {
            continue;
        };
        if object.get("kind").and_then(Value::as_str) != Some("task") {
            continue;
        }
        let Some(node_id) = object.get("id").and_then(Value::as_str) else {
            continue;
        };
        let Some(task_id) = task_id_map.get(node_id) else {
            continue;
        };
        object.insert("task_id".into(), Value::from(*task_id));
    }
}

fn stamp_workflow_task_metadata(task: &mut ScheduledTask, workflow_id: Option<i64>, node_id: &str) {
    if !task.metadata.is_object() {
        task.metadata = json!({});
    }
    let Some(metadata) = task.metadata.as_object_mut() else {
        return;
    };
    metadata.insert("task_type".into(), Value::String("workflow".into()));
    if !node_id.is_empty() {
        metadata.insert("workflow_node_id".into(), Value::String(node_id.into()));
    }
    if let Some(workflow_id) = workflow_id {
        metadata.insert("workflow_id".into(), Value::from(workflow_id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rewrites_temporary_workflow_task_ids_by_node_id() {
        let mut definition = json!({
            "nodes": [
                { "id": "build", "kind": "task", "task_id": -1 },
                { "id": "wait", "kind": "wait" }
            ]
        });
        let mut task_ids = HashMap::new();
        task_ids.insert("build".to_string(), 100);

        rewrite_workflow_task_ids(&mut definition, &task_ids);

        assert_eq!(definition["nodes"][0]["task_id"], 100);
        assert!(definition["nodes"][1].get("task_id").is_none());
    }

    #[test]
    fn stamps_workflow_task_metadata_without_clobbering_existing_values() {
        let mut task = ScheduledTask {
            id: Some(1),
            name: "task".into(),
            cron_schedule: "* * * * *".into(),
            action_name: "provider".into(),
            action_function: "run".into(),
            timeout: 1,
            next_execution: None,
            enabled: false,
            immediate: false,
            blackout_start: None,
            blackout_end: None,
            default_parameters: json!({}),
            mcp_enabled: false,
            metadata: json!({ "summary": "kept" }),
            tags: vec![],
        };

        stamp_workflow_task_metadata(&mut task, Some(7), "build");

        assert_eq!(task.metadata["summary"], "kept");
        assert_eq!(task.metadata["task_type"], "workflow");
        assert_eq!(task.metadata["workflow_id"], 7);
        assert_eq!(task.metadata["workflow_node_id"], "build");
    }
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
