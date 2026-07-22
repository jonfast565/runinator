use std::sync::Arc;
use uuid::Uuid;

use axum::{Extension, Json, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::{
    auth::{AuthContext, Permission},
    types::RuninatorType,
    value::Value,
    workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger},
};
use runinator_wdl::{CompileOptions, Severity, WdlError, WdlFragmentKind, WorkflowSignature};
use serde::{Deserialize, Serialize};

use crate::authz;
use crate::events::{EventSender, emit_workflows_changed};
use crate::handlers::providers::provider_metadata_from_items;
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request};

pub(crate) async fn complete_wdl(
    Json(request): Json<runinator_wdl::WdlCompletionRequest>,
) -> Json<runinator_wdl::WdlCompletionResponse> {
    Json(runinator_wdl::complete_source(request))
}

pub(crate) async fn hover_wdl(
    Json(request): Json<runinator_wdl::WdlHoverRequest>,
) -> Json<Option<runinator_wdl::WdlHoverResponse>> {
    Json(runinator_wdl::hover_source(request))
}

#[derive(Deserialize)]
pub(crate) struct CompileWdlRequest {
    pub source: String,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Deserialize)]
pub(crate) struct WdlSourceRequest {
    pub source: String,
    #[serde(default)]
    pub fragment: Option<WdlFragmentKind>,
}

#[derive(Deserialize)]
pub(crate) struct DecompileWdlRequest {
    pub workflow: WorkflowDefinition,
}

#[derive(Deserialize)]
pub(crate) struct EvaluateExpressionRequest {
    #[serde(default)]
    pub expression: Option<Value>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default = "default_fragment_kind")]
    pub kind: WdlFragmentKind,
    #[serde(default)]
    pub context: Value,
}

#[derive(Deserialize)]
pub(crate) struct ImportWdlRequest {
    pub source: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<Uuid>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
    #[serde(default)]
    pub ui: Option<Value>,
}

/// a wdl diagnostic flattened for the editor linter: byte offsets plus 1-based line/column.
#[derive(Serialize)]
pub(crate) struct DiagnosticSummary {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub column: usize,
    pub severity: String,
    pub message: String,
}

pub(crate) async fn compile_wdl<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<CompileWdlRequest>,
) -> Result<Json<WorkflowDefinition>, (StatusCode, String)> {
    let providers = fetch_provider_metadata(db.as_ref())
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    let workflow_signatures = workflow_signatures_for_compile(db.as_ref(), &request.source)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    let options = CompileOptions {
        enabled: request.enabled,
        providers,
        workflow_signatures,
        ..CompileOptions::default()
    };
    runinator_wdl::compile_str(&request.source, &options)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

pub(crate) async fn import_wdl<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<ImportWdlRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    // saving over an existing workflow requires edit; a brand-new one is owned by its creator.
    let is_create = request.workflow_id.is_none();
    if let Some(id) = request.workflow_id {
        if let Err(reply) = authz::require_workflow(db.as_ref(), &ctx, id, Permission::Edit).await {
            return reply;
        }
    }
    let providers = match fetch_provider_metadata(db.as_ref()).await {
        Ok(providers) => providers,
        Err(err) => return api_error(err),
    };
    let workflow_signatures =
        match workflow_signatures_for_compile(db.as_ref(), &request.source).await {
            Ok(signatures) => signatures,
            Err(err) => return api_error(err),
        };
    let options = CompileOptions {
        enabled: request.enabled,
        providers,
        workflow_signatures,
        ..CompileOptions::default()
    };
    let mut workflow = match runinator_wdl::compile_str(&request.source, &options) {
        Ok(workflow) => workflow,
        Err(err) => return bad_request(err.to_string()),
    };
    workflow.id = request.workflow_id;
    if let Some(ui) = request.ui
        && ui.is_object()
    {
        workflow.definition.extra.insert("ui".to_string(), ui);
    }
    let bundle = WorkflowBundle {
        workflows: vec![workflow],
        triggers: request.triggers,
    };
    match repository::import_workflow_bundle(db.as_ref(), bundle).await {
        Ok(saved) => {
            if is_create {
                for workflow in &saved.workflows {
                    if let Some(id) = workflow.id {
                        authz::grant_owner(db.as_ref(), &ctx, id).await;
                    }
                }
            }
            let org_id = saved
                .workflows
                .first()
                .and_then(|workflow| workflow.org_id)
                .or(ctx.org_id);
            emit_workflows_changed(&events, org_id);
            (StatusCode::OK, Json(ApiResponse::WorkflowBundle(saved)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn analyze_wdl<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<WdlSourceRequest>,
) -> Json<Vec<DiagnosticSummary>> {
    let source = request.source;
    let providers = fetch_provider_metadata(db.as_ref())
        .await
        .unwrap_or_default();
    if let Some(kind) = request.fragment {
        let options = CompileOptions {
            providers,
            ..CompileOptions::default()
        };
        return match runinator_wdl::validate_fragment(&source, kind, &options) {
            Ok(_) => Json(Vec::new()),
            Err(err) => Json(vec![wdl_error_to_summary(err, &source)]),
        };
    }
    // a parse failure is itself a finding, so surface it as a diagnostic instead of an error.
    let workflow_signatures = workflow_signatures_for_compile(db.as_ref(), &source)
        .await
        .unwrap_or_default();
    let diagnostics = match runinator_wdl::analyze_source_with_options(
        &source,
        &providers,
        runinator_wdl::TypePolicy::Strict,
        &workflow_signatures,
    ) {
        Ok(diagnostics) => diagnostics,
        Err(err) => return Json(vec![wdl_error_to_summary(err, &source)]),
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
    Json(summaries)
}

async fn fetch_provider_metadata<T: DatabaseImpl>(
    db: &T,
) -> Result<Vec<runinator_models::providers::ProviderMetadata>, String> {
    let items = repository::fetch_catalog_items(db, Some("provider_metadata".into()))
        .await
        .map_err(|err| err.to_string())?;
    provider_metadata_from_items(items).map_err(|err| err.to_string())
}

async fn workflow_signatures_for_compile<T: DatabaseImpl>(
    db: &T,
    source: &str,
) -> Result<Vec<WorkflowSignature>, String> {
    let mut signatures = db
        .fetch_workflows()
        .await
        .map_err(|err| err.to_string())?
        .iter()
        .flat_map(workflow_signatures_from_definition)
        .collect::<Vec<_>>();
    if let Ok(mut source_signatures) = runinator_wdl::workflow_signature_from_source(source) {
        signatures.append(&mut source_signatures);
    }
    Ok(signatures)
}

fn workflow_signatures_from_definition(workflow: &WorkflowDefinition) -> Vec<WorkflowSignature> {
    let input = workflow.input_type.clone();
    let output = RuninatorType::Any;
    let mut signatures = vec![WorkflowSignature {
        name: workflow.name.clone(),
        input: input.clone(),
        output: output.clone(),
    }];
    if let Some(namespace) = &workflow.namespace {
        signatures.push(WorkflowSignature {
            name: format!("{namespace}.{}", workflow.name),
            input,
            output,
        });
    }
    signatures
}

pub(crate) async fn format_wdl(
    Json(request): Json<WdlSourceRequest>,
) -> Result<Json<String>, (StatusCode, String)> {
    runinator_wdl::format_str(&request.source)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

pub(crate) async fn decompile_to_wdl(
    Json(request): Json<DecompileWdlRequest>,
) -> Result<Json<String>, (StatusCode, String)> {
    runinator_wdl::decompile(&request.workflow)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

/// resolve a lowered expression against a sample context for the editor's preview. mirrors the
/// desktop `evaluate_expression` command so the web client has the same behavior. evaluates the pure
/// compute tier (stdlib + higher-order intrinsics) but not effectful ops, so a preview never runs
/// side effects.
pub(crate) async fn evaluate_expression(
    Json(request): Json<EvaluateExpressionRequest>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if let Some(source) = request.source {
        return runinator_wdl::evaluate_fragment(
            &source,
            request.kind,
            &request.context,
            &CompileOptions::default(),
        )
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()));
    }
    let Some(expression) = request.expression else {
        return Err((
            StatusCode::BAD_REQUEST,
            "request must include either expression or source".into(),
        ));
    };
    evaluate_lowered_fragment(&expression, request.kind, &request.context)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err))
}

fn default_fragment_kind() -> WdlFragmentKind {
    WdlFragmentKind::Expression
}

fn evaluate_lowered_fragment(
    value: &Value,
    kind: WdlFragmentKind,
    context: &Value,
) -> Result<Value, String> {
    match kind {
        WdlFragmentKind::Expression => {
            runinator_workflows::validate_expression(value).map_err(|err| err.to_string())?;
            runinator_workflows::resolve_value_refs_pure(value, context)
                .map_err(|err| err.to_string())
        }
        WdlFragmentKind::Condition => {
            runinator_workflows::validate_condition_value(value).map_err(|err| err.to_string())?;
            runinator_workflows::evaluate_condition(value, context)
                .map(Value::Bool)
                .map_err(|err| err.to_string())
        }
        WdlFragmentKind::Compute => {
            let program =
                runinator_workflows::parse_program(value).map_err(|err| err.to_string())?;
            let outcome = runinator_workflows::run_program(
                &program,
                context,
                &runinator_workflows::PureIntrinsics,
            )
            .map_err(|err| err.to_string())?;
            Ok(compute_outcome_value(outcome))
        }
    }
}

fn compute_outcome_value(outcome: runinator_workflows::ComputeOutcome) -> Value {
    let mut map = runinator_models::value::Map::new();
    match outcome {
        runinator_workflows::ComputeOutcome::Return(value) => {
            map.insert("outcome".into(), Value::String("return".into()));
            map.insert("value".into(), value);
        }
        runinator_workflows::ComputeOutcome::Goto(target) => {
            map.insert("outcome".into(), Value::String("goto".into()));
            map.insert("target".into(), Value::String(target));
        }
        runinator_workflows::ComputeOutcome::Fallthrough(value) => {
            map.insert("outcome".into(), Value::String("fallthrough".into()));
            map.insert("value".into(), value);
        }
    }
    Value::Object(map)
}

/// flatten a `WdlError` into a single error diagnostic anchored to its span when it has one.
fn wdl_error_to_summary(err: WdlError, source: &str) -> DiagnosticSummary {
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
