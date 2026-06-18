use runinator_models::{
    api_routes::{
        API_WORKFLOWS_IMPORT, WORKFLOW_JSON_IMPORT_RISK_ACK, WORKFLOW_JSON_IMPORT_RISK_HEADER,
    },
    providers::ProviderMetadata,
    replicas::ReplicaListResponse,
    runs::{RunArtifact, RunChunk},
    web::TaskResponse,
    workflows::{
        WorkflowBundle, WorkflowDefinition, WorkflowNodeRunArtifact, WorkflowNodeRunChunk,
        WorkflowRun, WorkflowRunDeliverable, WorkflowTrigger,
    },
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, State};
use uuid::Uuid;

use crate::{
    client::{build_state_url, get_json, handle_response, post_empty, post_json},
    discovery::start_discovery_thread,
    error::{CommandError, CommandResult},
    state::CommandCenterState,
    types::{
        CredentialPutRequest, CredentialSummary, DiagnosticSummary, ServiceStatus,
        WorkflowRunCreated, WorkflowRunDetail,
    },
};
use runinator_wdl::{CompileOptions, Severity};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowWdlSaveRequest {
    pub source: String,
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
    #[serde(default)]
    pub ui: Option<Value>,
}

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

// ---- auth ----

#[tauri::command]
pub async fn auth_config(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "auth/config").await
}

#[tauri::command]
pub async fn auth_me(state: State<'_, CommandCenterState>) -> CommandResult<Value> {
    get_json(&state, "auth/me").await
}

#[tauri::command]
pub async fn login(
    state: State<'_, CommandCenterState>,
    username: String,
    password: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/login",
        &json!({ "username": username, "password": password }),
    )
    .await
}

#[tauri::command]
pub async fn refresh_session(
    state: State<'_, CommandCenterState>,
    refresh_token: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/refresh",
        &json!({ "refresh_token": refresh_token }),
    )
    .await
}

#[tauri::command]
pub async fn logout(
    state: State<'_, CommandCenterState>,
    refresh_token: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        "auth/logout",
        &json!({ "refresh_token": refresh_token }),
    )
    .await
}

/// store the access token so subsequent requests carry it (desktop side of the credential).
#[tauri::command]
pub async fn set_access_token(
    state: State<'_, CommandCenterState>,
    token: Option<String>,
) -> CommandResult<()> {
    state.set_access_token(token).await;
    Ok(())
}

#[tauri::command]
pub async fn list_workflow_grants(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
) -> CommandResult<Vec<Value>> {
    get_json(&state, &format!("workflows/{workflow_id}/grants")).await
}

#[tauri::command]
pub async fn create_workflow_grant(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
    principal_type: String,
    principal_id: Uuid,
    permission: String,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("workflows/{workflow_id}/grants"),
        &json!({
            "principal_type": principal_type,
            "principal_id": principal_id,
            "permission": permission,
        }),
    )
    .await
}

#[tauri::command]
pub async fn revoke_workflow_grant(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
    grant_id: Uuid,
) -> CommandResult<Value> {
    let url = build_state_url(
        &state,
        &format!("workflows/{workflow_id}/grants/{grant_id}"),
    )
    .await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
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
    let url = build_state_url(&state, API_WORKFLOWS_IMPORT).await?;
    println!(
        "Sending save_workflow_bundle to {}, workflow count: {}",
        url,
        request.workflows.len()
    );
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .header(
            WORKFLOW_JSON_IMPORT_RISK_HEADER,
            WORKFLOW_JSON_IMPORT_RISK_ACK,
        )
        .json(&request)
        .send()
        .await
        .map_err(|err| {
            eprintln!("Error sending request to {}: {}", url, err);
            err
        })?;
    let response = handle_response(url, response).await?;
    let result = response.json::<WorkflowBundle>().await?;
    let Some(workflow_id) = result.workflows.first().and_then(|workflow| workflow.id) else {
        return Ok(result);
    };
    get_json(&state, &format!("workflows/{workflow_id}/export")).await
}

#[tauri::command]
pub async fn save_workflow_wdl(
    state: State<'_, CommandCenterState>,
    request: WorkflowWdlSaveRequest,
) -> CommandResult<WorkflowBundle> {
    let url = build_state_url(&state, "wdl/import").await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&request)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    let result = response.json::<WorkflowBundle>().await?;
    let Some(workflow_id) = result.workflows.first().and_then(|workflow| workflow.id) else {
        return Ok(result);
    };
    get_json(&state, &format!("workflows/{workflow_id}/export")).await
}

#[tauri::command]
pub fn compile_wdl(source: String, enabled: bool) -> CommandResult<WorkflowDefinition> {
    let options = CompileOptions {
        enabled,
        providers: runinator_provider_catalog::metadata(),
        ..CompileOptions::default()
    };
    runinator_wdl::compile_str(&source, &options)
        .map_err(|err| CommandError::Unexpected(err.to_string()))
}

#[tauri::command]
pub fn analyze_wdl(source: String) -> CommandResult<Vec<DiagnosticSummary>> {
    let providers = runinator_provider_catalog::metadata();
    // a parse failure is itself a finding, so surface it as a diagnostic instead of an error.
    let diagnostics = match runinator_wdl::analyze_source_with_providers(&source, &providers) {
        Ok(diagnostics) => diagnostics,
        Err(err) => return Ok(vec![wdl_error_to_summary(err, &source)]),
    };
    let summaries = diagnostics
        .into_iter()
        .map(|diagnostic| {
            let (line, column) = diagnostic.span.line_col(&source);
            let severity = match diagnostic.severity {
                Severity::Error => "error",
                Severity::Warning => "warning",
            };
            DiagnosticSummary {
                start: diagnostic.span.start,
                end: diagnostic.span.end,
                line,
                column,
                severity: severity.to_string(),
                message: diagnostic.message,
            }
        })
        .collect();
    Ok(summaries)
}

#[tauri::command]
pub fn complete_wdl(
    request: runinator_wdl::WdlCompletionRequest,
) -> CommandResult<runinator_wdl::WdlCompletionResponse> {
    Ok(runinator_wdl::complete_source(request))
}

#[tauri::command]
pub fn format_wdl(source: String) -> CommandResult<String> {
    runinator_wdl::format_str(&source).map_err(|err| CommandError::Unexpected(err.to_string()))
}

/// resolve a lowered WDL expression against a sample context (e.g. a prior run's data) so the editor
/// can preview the value a reference/transform/compute expression produces. evaluates the pure
/// compute tier (stdlib + higher-order intrinsics) but not effectful ops, so a preview never runs
/// side effects; an unresolvable reference or effectful call surfaces as a command error.
#[tauri::command]
pub fn evaluate_expression(expression: Value, context: Value) -> CommandResult<Value> {
    let expr = runinator_models::value::Value::from(expression);
    let ctx = runinator_models::value::Value::from(context);
    let resolved = runinator_workflows::resolve_value_refs_pure(&expr, &ctx)
        .map_err(|err| CommandError::Unexpected(err.to_string()))?;
    serde_json::to_value(&resolved).map_err(|err| CommandError::Unexpected(err.to_string()))
}

/// flatten a `WdlError` into a single error diagnostic anchored to its span when it has one.
fn wdl_error_to_summary(err: runinator_wdl::WdlError, source: &str) -> DiagnosticSummary {
    use runinator_wdl::WdlError;
    let span = match &err {
        WdlError::Syntax { span, .. } | WdlError::Semantic { span, .. } => Some(*span),
        _ => None,
    };
    let (start, end, line, column) = match span {
        Some(span) => {
            let (line, column) = span.line_col(source);
            (span.start, span.end, line, column)
        }
        None => (0, 0, 1, 1),
    };
    DiagnosticSummary {
        start,
        end,
        line,
        column,
        severity: "error".to_string(),
        message: err.to_string(),
    }
}

#[tauri::command]
pub fn decompile_to_wdl(workflow: WorkflowDefinition) -> CommandResult<String> {
    runinator_wdl::decompile(&workflow).map_err(|err| CommandError::Unexpected(err.to_string()))
}

#[tauri::command]
pub async fn delete_workflow(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflows/{workflow_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn duplicate_workflow(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
    bump: Option<String>,
) -> CommandResult<WorkflowDefinition> {
    let bump = bump.unwrap_or_else(|| "minor".into());
    let url = build_state_url(
        &state,
        &format!("workflows/{workflow_id}/duplicate?bump={bump}"),
    )
    .await?;
    let response = state.client.read().await.post(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<WorkflowDefinition>().await?)
}

#[tauri::command]
pub async fn fetch_workflow_triggers(
    state: State<'_, CommandCenterState>,
    workflow_id: Uuid,
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
    Ok(response.json::<WorkflowTrigger>().await?)
}

#[tauri::command]
pub async fn delete_workflow_trigger(
    state: State<'_, CommandCenterState>,
    trigger_id: Uuid,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_triggers/{trigger_id}")).await?;
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

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
pub async fn fetch_workflow_node_run_chunks(
    state: State<'_, CommandCenterState>,
    node_run_id: Uuid,
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
    node_run_id: Uuid,
) -> CommandResult<Vec<WorkflowNodeRunArtifact>> {
    get_json(
        &state,
        &format!("workflow_node_runs/{node_run_id}/artifacts"),
    )
    .await
}

#[tauri::command]
pub async fn fetch_workflow_run_deliverables(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
) -> CommandResult<Vec<WorkflowRunDeliverable>> {
    get_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/deliverables"),
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
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}

#[tauri::command]
pub async fn continue_workflow_run(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
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
        .json(&json!({}))
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
pub async fn resolve_workflow_input(
    state: State<'_, CommandCenterState>,
    node_run_id: Uuid,
    output_json: Value,
    resolved_by: Option<String>,
    message: Option<String>,
) -> CommandResult<TaskResponse> {
    let url = build_state_url(&state, &format!("workflow_node_runs/{node_run_id}/input")).await?;
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&json!({
            "output_json": output_json,
            "resolved_by": resolved_by,
            "message": message
        }))
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
pub async fn fetch_replicas(
    state: State<'_, CommandCenterState>,
) -> CommandResult<ReplicaListResponse> {
    get_json(&state, "replicas").await
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
    let response = state
        .client
        .read()
        .await
        .post(url.clone())
        .json(&request)
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn delete_credential(
    state: State<'_, CommandCenterState>,
    scope: String,
    name: String,
    kind: Option<String>,
) -> CommandResult<Value> {
    let mut url = build_state_url(&state, "credentials").await?;
    url.query_pairs_mut()
        .append_pair("scope", &scope)
        .append_pair("name", &name)
        .append_pair("kind", kind.as_deref().unwrap_or("secret"));
    let response = state.client.read().await.delete(url.clone()).send().await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<Value>().await?)
}

#[tauri::command]
pub async fn approve_approval(
    state: State<'_, CommandCenterState>,
    approval_id: Uuid,
) -> CommandResult<Value> {
    post_empty(&state, &format!("approvals/{approval_id}/approve")).await
}

#[tauri::command]
pub async fn reject_approval(
    state: State<'_, CommandCenterState>,
    approval_id: Uuid,
) -> CommandResult<Value> {
    post_empty(&state, &format!("approvals/{approval_id}/reject")).await
}

#[tauri::command]
pub async fn open_gate(
    state: State<'_, CommandCenterState>,
    gate_id: Uuid,
    reason: Option<String>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("gates/{gate_id}/open"),
        &json!({ "reason": reason }),
    )
    .await
}

#[tauri::command]
pub async fn close_gate(
    state: State<'_, CommandCenterState>,
    gate_id: Uuid,
    reason: Option<String>,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("gates/{gate_id}/close"),
        &json!({ "reason": reason }),
    )
    .await
}

#[tauri::command]
pub async fn deliver_signal(
    state: State<'_, CommandCenterState>,
    workflow_run_id: Uuid,
    name: String,
    payload: Value,
) -> CommandResult<Value> {
    post_json(
        &state,
        &format!("workflow_runs/{workflow_run_id}/signals"),
        &json!({ "name": name, "payload": payload }),
    )
    .await
}

#[tauri::command]
pub async fn fetch_all_artifacts(
    state: State<'_, CommandCenterState>,
) -> CommandResult<Vec<RunArtifact>> {
    get_json(&state, "artifacts").await
}

#[derive(serde::Deserialize)]
pub struct ArtifactUploadRequest {
    pub run_id: Uuid,
    #[serde(default)]
    pub workflow_node_run_id: Option<Uuid>,
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
        .read()
        .await
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
    artifact_id: Uuid,
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
    let response = state.client.read().await.get(url.clone()).send().await?;
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
    notification_id: Uuid,
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
        .read()
        .await
        .post(url.clone())
        .json(&json!({}))
        .send()
        .await?;
    let response = handle_response(url, response).await?;
    Ok(response.json::<TaskResponse>().await?)
}
