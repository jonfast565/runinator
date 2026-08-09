use super::*;

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
    super::runs::save_workflow_to_service(&state, &workflow).await
}

#[tauri::command]
pub async fn simulate_workflow(
    state: State<'_, CommandCenterState>,
    request: WorkflowSimulateRequest,
) -> CommandResult<Value> {
    let body =
        serde_json::to_value(&request).map_err(|err| CommandError::Unexpected(err.to_string()))?;
    post_json(&state, API_WORKFLOWS_SIMULATE, &body).await
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
pub fn analyze_wdl(
    source: String,
    source_path: Option<String>,
) -> CommandResult<Vec<DiagnosticSummary>> {
    let providers = runinator_provider_catalog::metadata();
    let workflow_signatures = source_path
        .as_deref()
        .map(std::path::Path::new)
        .map(|path| crate::pack_dev::wdl_context_workflow_signatures(path, Some(&source)))
        .transpose()?
        .unwrap_or_default();
    // a parse failure is itself a finding, so surface it as a diagnostic instead of an error.
    let diagnostics = match runinator_wdl::analyze_source_with_options(
        &source,
        &providers,
        runinator_wdl::TypePolicy::Strict,
        &workflow_signatures,
    ) {
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
    request: runinator_wdl_ide::WdlCompletionRequest,
) -> CommandResult<runinator_wdl_ide::WdlCompletionResponse> {
    Ok(runinator_wdl_ide::complete_source(request))
}

#[tauri::command]
pub fn hover_wdl(
    request: runinator_wdl_ide::WdlHoverRequest,
) -> CommandResult<Option<runinator_wdl_ide::WdlHoverResponse>> {
    Ok(runinator_wdl_ide::hover_source(request))
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
