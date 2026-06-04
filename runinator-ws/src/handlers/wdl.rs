use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::workflows::{WorkflowBundle, WorkflowDefinition, WorkflowTrigger};
use runinator_wdl::{CompileOptions, Severity, WdlError};
use serde::{Deserialize, Serialize};

use crate::events::{AppEvent, EventSender, emit};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request};

pub(crate) async fn complete_wdl(
    Json(request): Json<runinator_wdl::WdlCompletionRequest>,
) -> Json<runinator_wdl::WdlCompletionResponse> {
    Json(runinator_wdl::complete_source(request))
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
}

#[derive(Deserialize)]
pub(crate) struct DecompileWdlRequest {
    pub workflow: WorkflowDefinition,
}

#[derive(Deserialize)]
pub(crate) struct ImportWdlRequest {
    pub source: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub workflow_id: Option<i64>,
    #[serde(default)]
    pub triggers: Vec<WorkflowTrigger>,
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

pub(crate) async fn compile_wdl(
    Json(request): Json<CompileWdlRequest>,
) -> Result<Json<WorkflowDefinition>, (StatusCode, String)> {
    let options = CompileOptions {
        enabled: request.enabled,
        ..CompileOptions::default()
    };
    runinator_wdl::compile_str(&request.source, &options)
        .map(Json)
        .map_err(|err| (StatusCode::BAD_REQUEST, err.to_string()))
}

pub(crate) async fn import_wdl<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Json(request): Json<ImportWdlRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let options = CompileOptions {
        enabled: request.enabled,
        ..CompileOptions::default()
    };
    let mut workflow = match runinator_wdl::compile_str(&request.source, &options) {
        Ok(workflow) => workflow,
        Err(err) => return bad_request(err.to_string()),
    };
    workflow.id = request.workflow_id;
    let bundle = WorkflowBundle {
        workflows: vec![workflow],
        triggers: request.triggers,
    };
    match repository::import_workflow_bundle(db.as_ref(), bundle).await {
        Ok(saved) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowBundle(saved)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn analyze_wdl(
    Json(request): Json<WdlSourceRequest>,
) -> Json<Vec<DiagnosticSummary>> {
    let source = request.source;
    // a parse failure is itself a finding, so surface it as a diagnostic instead of an error.
    let diagnostics = match runinator_wdl::analyze_source(&source) {
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
