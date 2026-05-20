mod config;
mod models;
mod repository;
#[cfg(test)]
mod tests;

use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};

use axum::{
    Extension, Json, Router,
    extract::{
        Path, Query,
        ws::{Message, WebSocketUpgrade},
    },
    http::StatusCode,
    response::Response,
    routing::{get, patch, post},
};
use futures::{SinkExt, StreamExt};
use log::info;
use models::{
    ApiError, ApiResponse, ApprovalResolutionRequest, AutomationRecordQuery, CatalogQuery,
    CredentialPutRequest, CredentialQuery, IdempotencyRequest, WebhookWakeRequest,
    WorkflowNodeRunRequest, WorkflowNodeRunStatusRequest, WorkflowRunRequest,
    WorkflowRunStatusQuery, WorkflowRunStatusRequest, WorkflowTriggerRunRequest,
};
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::{
    errors::SendableError,
    providers::ProviderMetadata,
    runs::{NewRunArtifact, NewRunChunk},
    web::TaskResponse,
    workflows::{WorkflowDefinition, WorkflowTrigger},
};
use runinator_utilities::credential_store::{
    CredentialStore, LocalEncryptedCredentialStore, default_credential_store_path,
};
use serde::{Deserialize, Serialize};
use tokio::{
    net::TcpListener,
    sync::{Notify, broadcast},
};

#[derive(Clone, Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    RunStatusChanged { run_id: i64 },
    RunChunkAdded { run_id: i64 },
    WorkflowsChanged,
    WorkflowRunChanged { run_id: i64 },
    WorkflowRunActivity,
}

type EventSender = broadcast::Sender<AppEvent>;

fn emit(events: &EventSender, event: AppEvent) {
    let _ = events.send(event);
}

fn emit_workflow_run(events: &EventSender, run_id: i64) {
    emit(events, AppEvent::WorkflowRunChanged { run_id });
}

async fn emit_workflow_node_run<T: DatabaseImpl>(
    db: &T,
    events: &EventSender,
    workflow_node_run_id: i64,
) {
    if let Ok(Some(node_run)) = repository::fetch_workflow_node_run(db, workflow_node_run_id).await
    {
        emit_workflow_run(events, node_run.workflow_run_id);
    }
}

#[derive(Debug, Default, Deserialize)]
struct ChunkQuery {
    cursor: Option<i64>,
    limit: Option<i64>,
}

fn api_error(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::ApiError(ApiError {
            message: message.into(),
        })),
    )
}

fn not_found(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::ApiError(ApiError {
            message: message.into(),
        })),
    )
}

fn bad_request(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiResponse::ApiError(ApiError {
            message: message.into(),
        })),
    )
}

fn task_response_success(message: impl Into<String>) -> (StatusCode, Json<ApiResponse>) {
    (
        StatusCode::OK,
        Json(ApiResponse::TaskResponse(TaskResponse {
            success: true,
            message: message.into(),
        })),
    )
}

async fn upsert_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Json(workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::upsert_workflow(db.as_ref(), &workflow).await {
        Ok(workflow) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::Workflow(workflow)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflows<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflows(db.as_ref()).await {
        Ok(workflows) => (StatusCode::OK, Json(ApiResponse::WorkflowList(workflows))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow(db.as_ref(), workflow_id).await {
        Ok(Some(workflow)) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
        Ok(None) => not_found(format!("Workflow {workflow_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

async fn delete_workflow<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::delete_workflow(db.as_ref(), workflow_id).await {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn upsert_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_id): Path<i64>,
    Json(mut trigger): Json<WorkflowTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    trigger.workflow_id = workflow_id;
    match repository::upsert_workflow_trigger(db.as_ref(), &trigger).await {
        Ok(trigger) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(trigger_id): Path<i64>,
    Json(mut trigger): Json<WorkflowTrigger>,
) -> (StatusCode, Json<ApiResponse>) {
    trigger.id = Some(trigger_id);
    match repository::upsert_workflow_trigger(db.as_ref(), &trigger).await {
        Ok(trigger) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(trigger_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_trigger(db.as_ref(), trigger_id).await {
        Ok(Some(trigger)) => (StatusCode::OK, Json(ApiResponse::WorkflowTrigger(trigger))),
        Ok(None) => not_found(format!("Workflow trigger {trigger_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_triggers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_triggers(db.as_ref(), workflow_id).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_due_workflow_triggers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_due_workflow_triggers(db.as_ref()).await {
        Ok(triggers) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowTriggerList(triggers)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn delete_workflow_trigger<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(trigger_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::delete_workflow_trigger(db.as_ref(), trigger_id).await {
        Ok(resp) => {
            emit(&events, AppEvent::WorkflowsChanged);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn create_workflow_trigger_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(trigger_id): Path<i64>,
    Json(request): Json<WorkflowTriggerRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_run_for_trigger(
        db.as_ref(),
        trigger_id,
        request.parameters,
        request.debug,
    )
    .await
    {
        Ok(run) => {
            emit_workflow_run(&events, run.id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                    run,
                    nodes: Vec::new(),
                })),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn create_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_id): Path<i64>,
    Json(request): Json<WorkflowRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_run(
        db.as_ref(),
        workflow_id,
        request.parameters,
        request.debug,
    )
    .await
    {
        Ok(run) => {
            emit_workflow_run(&events, run.id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                    run,
                    nodes: Vec::new(),
                })),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn step_debug_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::step_debug_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

async fn continue_debug_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::continue_debug_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

async fn update_workflow_run_debug<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(patch): Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_workflow_run_debug(db.as_ref(), workflow_run_id, patch).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

async fn cancel_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::cancel_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

#[derive(serde::Deserialize)]
struct RunToCursorRequest {
    node_id: String,
}

async fn run_to_cursor_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(req): Json<RunToCursorRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::run_to_cursor_workflow_run(db.as_ref(), workflow_run_id, req.node_id).await {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

#[derive(serde::Deserialize)]
struct SkipDebugRequest {
    output_json: serde_json::Value,
    message: Option<String>,
}

async fn skip_debug_workflow_node<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(req): Json<SkipDebugRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::skip_debug_workflow_node(
        db.as_ref(),
        workflow_run_id,
        req.output_json,
        req.message,
    )
    .await
    {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

#[derive(serde::Deserialize)]
struct RerunNodeRequest {
    parameters: serde_json::Value,
}

async fn rerun_debug_workflow_node<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(req): Json<RerunNodeRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::rerun_debug_workflow_node(db.as_ref(), workflow_run_id, req.parameters).await
    {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => bad_request(err.to_string()),
    }
}

async fn replay_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::replay_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(run) => {
            emit(&events, AppEvent::WorkflowRunChanged { run_id: run.id });
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                    run,
                    nodes: Vec::new(),
                })),
            )
        }
        Err(err) => bad_request(err.to_string()),
    }
}

async fn get_supervisor_status() -> (StatusCode, Json<serde_json::Value>) {
    let path = std::env::var("RUNINATOR_SUPERVISOR_STATE_PATH")
        .unwrap_or_else(|_| "./.runinator-supervisor/state.json".to_string());
    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "configured": false,
                "path": path
            })),
        );
    }
    match runinator_supervisor::snapshot::read_snapshot(&path_buf) {
        Ok(snapshot) => {
            let stale_seconds = compute_stale_seconds(&snapshot.updated_at);
            let mut body = serde_json::to_value(&snapshot).unwrap_or_else(|_| serde_json::json!({}));
            if let Some(obj) = body.as_object_mut() {
                obj.insert("stale_seconds".into(), serde_json::json!(stale_seconds));
                obj.insert("configured".into(), serde_json::json!(true));
            }
            (StatusCode::OK, Json(body))
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "configured": true,
                "error": err.to_string()
            })),
        ),
    }
}

fn compute_stale_seconds(updated_at: &str) -> Option<i64> {
    let parsed = chrono::DateTime::parse_from_rfc3339(updated_at).ok()?;
    let now = chrono::Utc::now();
    Some((now - parsed.with_timezone(&chrono::Utc)).num_seconds())
}

async fn get_workflow_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<WorkflowRunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(workflow_id) = query.workflow_id {
        return match repository::fetch_workflow_runs_for_workflow(db.as_ref(), workflow_id).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    if let Some(status) = query.status {
        return match repository::fetch_workflow_runs_by_status(db.as_ref(), status).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
            Err(err) => api_error(err.to_string()),
        };
    }

    match repository::fetch_recent_workflow_runs(db.as_ref()).await {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<WorkflowRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_workflow_run_status(
        db.as_ref(),
        workflow_run_id,
        request.status,
        request.active_node_id,
        request.state,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit(
                &events,
                AppEvent::WorkflowRunChanged {
                    run_id: workflow_run_id,
                },
            );
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(Some((run, nodes))) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                run,
                nodes,
            })),
        ),
        Ok(None) => not_found(format!("Workflow run {workflow_run_id} not found")),
        Err(err) => api_error(err.to_string()),
    }
}

async fn create_workflow_node_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<WorkflowNodeRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_node_run(
        db.as_ref(),
        workflow_run_id,
        request.node_id,
        request.parameters,
    )
    .await
    {
        Ok(step) => {
            emit_workflow_run(&events, workflow_run_id);
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRun(step)),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_workflow_node_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(node_run_id): Path<i64>,
    Json(request): Json<WorkflowNodeRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_workflow_node_run(
        db.as_ref(),
        node_run_id,
        request.status,
        request.attempt,
        request.parameters,
        request.output_json,
        request.state,
        request.transition_reason,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_node_run_chunks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(node_run_id): Path<i64>,
    Query(query): Query<ChunkQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_node_run_chunks(
        db.as_ref(),
        node_run_id,
        query.cursor,
        query.limit.unwrap_or(100),
    )
    .await
    {
        Ok(chunks) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowNodeRunChunks(chunks)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn append_workflow_node_run_chunk<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(node_run_id): Path<i64>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::append_workflow_node_run_chunk(db.as_ref(), node_run_id, &chunk).await {
        Ok(chunk) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRunChunks(vec![chunk])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_node_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(node_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_node_run_artifacts(db.as_ref(), node_run_id).await {
        Ok(artifacts) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowNodeRunArtifacts(artifacts)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn add_workflow_node_run_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(node_run_id): Path<i64>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::add_workflow_node_run_artifact(db.as_ref(), node_run_id, &artifact).await {
        Ok(artifact) => {
            emit_workflow_node_run(db.as_ref(), &events, node_run_id).await;
            (
                StatusCode::ACCEPTED,
                Json(ApiResponse::WorkflowNodeRunArtifacts(vec![artifact])),
            )
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_catalog_items<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<CatalogQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Some(uri) = query.uri {
        return match repository::fetch_catalog_item(db.as_ref(), uri.clone()).await {
            Ok(Some(item)) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
            Ok(None) => not_found(format!("Catalog item {uri} not found")),
            Err(err) => api_error(err.to_string()),
        };
    }
    match repository::fetch_catalog_items(db.as_ref(), query.item_type).await {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::JsonList(items))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn upsert_catalog_item<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(item): Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::upsert_catalog_item(db.as_ref(), item).await {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn list_records<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<AutomationRecordQuery>,
    record_type: &'static str,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_automation_records(
        db.as_ref(),
        record_type,
        query.workflow_run_id,
        query.external_item_id,
    )
    .await
    {
        Ok(records) => (StatusCode::OK, Json(ApiResponse::JsonList(records))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn create_record<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    record_type: &'static str,
    Json(record): Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_automation_record(db.as_ref(), record_type, record).await {
        Ok(record) => (StatusCode::ACCEPTED, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_external_items<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "external_items").await
}

async fn create_external_item<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "external_items", json).await
}

async fn get_external_resources<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "external_resources").await
}

async fn create_external_resource<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "external_resources", json).await
}

async fn get_feedback<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "feedback").await
}

async fn create_feedback<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "feedback", json).await
}

async fn get_gates<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "gates").await
}

async fn create_gate<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "gates", json).await
}

async fn get_workspaces<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "workspaces").await
}

async fn create_workspace<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "workspaces", json).await
}

async fn get_change_sets<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "change_sets").await
}

async fn create_change_set<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "change_sets", json).await
}

async fn get_automation_events<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "automation_events").await
}

async fn create_automation_event<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "automation_events", json).await
}

async fn get_approvals<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    query: Query<AutomationRecordQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    list_records(ext, query, "approval_requests").await
}

async fn create_approval<T: DatabaseImpl>(
    ext: Extension<Arc<T>>,
    json: Json<serde_json::Value>,
) -> (StatusCode, Json<ApiResponse>) {
    create_record(ext, "approval_requests", json).await
}

async fn approve_request<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(approval_id): Path<i64>,
    Json(request): Json<ApprovalResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::resolve_approval(
        db.as_ref(),
        approval_id,
        true,
        request.resolved_by,
        request.message,
        request.output_json,
    )
    .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn reject_request<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(approval_id): Path<i64>,
    Json(request): Json<ApprovalResolutionRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::resolve_approval(
        db.as_ref(),
        approval_id,
        false,
        request.resolved_by,
        request.message,
        request.output_json,
    )
    .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<HashMap<String, String>>,
) -> (StatusCode, Json<ApiResponse>) {
    let Some(scope) = query.get("scope").cloned() else {
        return api_error("idempotency query requires scope");
    };
    let Some(key) = query.get("key").cloned() else {
        return api_error("idempotency query requires key");
    };
    match repository::fetch_idempotency_key(db.as_ref(), scope, key).await {
        Ok(Some(record)) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Ok(None) => not_found("idempotency key not found"),
        Err(err) => api_error(err.to_string()),
    }
}

async fn put_idempotency_key<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(request): Json<IdempotencyRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::put_idempotency_key(db.as_ref(), request.scope, request.key, request.result)
        .await
    {
        Ok(record) => (StatusCode::OK, Json(ApiResponse::JsonValue(record))),
        Err(err) => api_error(err.to_string()),
    }
}

fn credential_store() -> LocalEncryptedCredentialStore {
    let path = std::env::var("RUNINATOR_CREDENTIAL_STORE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| default_credential_store_path(".runinator-supervisor"));
    let key = std::env::var("RUNINATOR_CREDENTIAL_KEY")
        .unwrap_or_else(|_| "runinator-local-development-key".into());
    LocalEncryptedCredentialStore::new(path, key)
}

async fn get_credential(Query(query): Query<CredentialQuery>) -> (StatusCode, Json<ApiResponse>) {
    let store = credential_store();
    if query.scope.is_none() && query.name.is_none() {
        return match store.list() {
            Ok(entries) => (
                StatusCode::OK,
                Json(ApiResponse::JsonList(
                    entries
                        .into_iter()
                        .map(|entry| {
                            serde_json::json!({
                                "scope": entry.scope,
                                "name": entry.name,
                            })
                        })
                        .collect(),
                )),
            ),
            Err(err) => api_error(err.to_string()),
        };
    }

    let (Some(scope), Some(name)) = (query.scope, query.name) else {
        return bad_request("credential lookup requires both scope and name");
    };

    match store.get(&scope, &name) {
        Ok(Some(secret)) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(serde_json::json!({
                "scope": scope,
                "name": name,
                "secret": String::from_utf8_lossy(&secret)
            }))),
        ),
        Ok(None) => not_found("credential not found"),
        Err(err) => api_error(err.to_string()),
    }
}

async fn put_credential(
    Json(request): Json<CredentialPutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match credential_store().put(&request.scope, &request.name, request.secret.as_bytes()) {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(serde_json::json!({
                "scope": request.scope,
                "name": request.name,
                "stored": true
            }))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn delete_credential(
    Query(query): Query<CredentialQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    let (Some(scope), Some(name)) = (query.scope, query.name) else {
        return bad_request("credential deletion requires both scope and name");
    };

    match credential_store().delete(&scope, &name) {
        Ok(()) => (
            StatusCode::OK,
            Json(ApiResponse::TaskResponse(TaskResponse {
                success: true,
                message: "Credential deleted".into(),
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_providers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    let items = match repository::fetch_catalog_items(db.as_ref(), Some("provider_metadata".into()))
        .await
    {
        Ok(items) => items,
        Err(err) => return api_error(err.to_string()),
    };

    match provider_metadata_from_items(items) {
        Ok(providers) => (StatusCode::OK, Json(ApiResponse::ProviderList(providers))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn upsert_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(provider): Json<ProviderMetadata>,
) -> (StatusCode, Json<ApiResponse>) {
    let item = provider_catalog_item(&provider);
    let item = match repository::upsert_catalog_item(db.as_ref(), item).await {
        Ok(item) => item,
        Err(err) => return api_error(err.to_string()),
    };

    match provider_metadata_from_item(item) {
        Ok(provider) => (StatusCode::OK, Json(ApiResponse::Provider(provider))),
        Err(err) => api_error(err.to_string()),
    }
}

fn provider_metadata_from_items(
    items: Vec<serde_json::Value>,
) -> Result<Vec<ProviderMetadata>, serde_json::Error> {
    let mut providers = items
        .into_iter()
        .map(provider_metadata_from_item)
        .collect::<Result<Vec<_>, _>>()?;
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(providers)
}

fn provider_metadata_from_item(
    item: serde_json::Value,
) -> Result<ProviderMetadata, serde_json::Error> {
    let document = item.get("document").cloned().unwrap_or(item);
    serde_json::from_value(document)
}

fn provider_catalog_item(provider: &ProviderMetadata) -> serde_json::Value {
    serde_json::json!({
        "uri": format!("runinator://providers/{}", provider.name),
        "item_type": "provider_metadata",
        "name": provider.name,
        "version": "1",
        "document": provider,
        "metadata": {}
    })
}

async fn webhook_wake<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Json(request): Json<WebhookWakeRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let workflow_run =
        match repository::fetch_workflow_run(db.as_ref(), request.workflow_run_id).await {
            Ok(Some(workflow_run)) => workflow_run,
            Ok(None) => {
                return not_found(format!(
                    "Workflow run {} not found",
                    request.workflow_run_id
                ));
            }
            Err(err) => return api_error(err.to_string()),
        };
    let (run, node_runs) = workflow_run;
    let node_id = request
        .node_id
        .clone()
        .or(run.active_node_id)
        .unwrap_or_default();
    let Some(node_run) = node_runs
        .iter()
        .filter(|node_run| node_run.node_id == node_id)
        .max_by_key(|node_run| node_run.id)
    else {
        return task_response_success("Webhook wake recorded");
    };

    let mut state = node_run.state.clone();
    merge_json(&mut state, request.state);
    if let Some(status) = request.status {
        if let Some(object) = state.as_object_mut() {
            object.insert("status".into(), status.into());
        }
    }
    if let Err(err) = repository::update_workflow_node_run(
        db.as_ref(),
        node_run.id,
        runinator_models::workflows::WorkflowStatus::Waiting,
        None,
        None,
        None,
        Some(state.clone()),
        Some("webhook_wake".into()),
        None,
    )
    .await
    {
        return api_error(err.to_string());
    }
    if let Err(err) = repository::update_workflow_run_status(
        db.as_ref(),
        request.workflow_run_id,
        runinator_models::workflows::WorkflowStatus::Waiting,
        Some(node_id),
        Some(state),
        request.message,
    )
    .await
    {
        return api_error(err.to_string());
    }
    emit_workflow_run(&events, request.workflow_run_id);
    task_response_success("Webhook wake recorded")
}

async fn send_json<T: Serialize>(
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    value: &T,
) -> Result<(), ()> {
    let payload = serde_json::to_string(value).map_err(|_| ())?;
    tx.send(Message::Text(payload.into())).await.map_err(|_| ())
}

async fn send_run_chunks<T: DatabaseImpl>(
    db: &T,
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    run_id: i64,
    cursor: &mut Option<i64>,
    limit: i64,
) -> Result<(), ()> {
    let chunks = repository::fetch_run_chunks(db, run_id, *cursor, limit)
        .await
        .map_err(|_| ())?;
    for chunk in &chunks {
        send_json(tx, chunk).await?;
        *cursor = Some(chunk.sequence);
    }
    Ok(())
}

async fn send_workflow_node_run_chunks<T: DatabaseImpl>(
    db: &T,
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    node_run_id: i64,
    cursor: &mut Option<i64>,
    limit: i64,
) -> Result<(), ()> {
    let chunks = repository::fetch_workflow_node_run_chunks(db, node_run_id, *cursor, limit)
        .await
        .map_err(|_| ())?;
    for chunk in &chunks {
        send_json(tx, chunk).await?;
        *cursor = Some(chunk.sequence);
    }
    Ok(())
}

async fn send_workflow_run<T: DatabaseImpl>(
    db: &T,
    tx: &mut futures::stream::SplitSink<axum::extract::ws::WebSocket, Message>,
    run_id: i64,
) -> Result<bool, ()> {
    let Some((run, nodes)) = repository::fetch_workflow_run(db, run_id)
        .await
        .map_err(|_| ())?
    else {
        return Err(());
    };
    let terminal = run.status.is_terminal();
    send_json(tx, &models::WorkflowRunResponse { run, nodes }).await?;
    Ok(terminal)
}

fn merge_json(target: &mut serde_json::Value, overlay: serde_json::Value) {
    match (target, overlay) {
        (serde_json::Value::Object(target), serde_json::Value::Object(overlay)) => {
            for (key, value) in overlay {
                match target.get_mut(&key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        target.insert(key, value);
                    }
                }
            }
        }
        (target, overlay) => *target = overlay,
    }
}

async fn ws_events(Extension(events): Extension<EventSender>, ws: WebSocketUpgrade) -> Response {
    let mut rx = events.subscribe();
    ws.on_upgrade(move |socket| async move {
        let (mut tx, _rx) = socket.split();
        while let Ok(event) = rx.recv().await {
            if send_json(&mut tx, &event).await.is_err() {
                break;
            }
        }
    })
}

async fn ws_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx_ws) = socket.split();
        let _ = send_workflow_run(db.as_ref(), &mut tx, run_id).await;
        let mut event_rx = events.subscribe();
        loop {
            tokio::select! {
                Ok(event) = event_rx.recv() => {
                    let relevant = matches!(&event,
                        AppEvent::WorkflowRunChanged { run_id: id } if *id == run_id
                    );
                    if !relevant {
                        continue;
                    }
                    let Ok(terminal) = send_workflow_run(db.as_ref(), &mut tx, run_id).await else {
                        break;
                    };
                    if terminal {
                        break;
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => break,
                        _ => {}
                    }
                }
            }
        }
    })
}

async fn ws_workflow_node_run_stream<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(node_run_id): Path<i64>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx_ws) = socket.split();
        let mut cursor: Option<i64> = None;
        if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 500)
            .await
            .is_err()
        {
            return;
        }
        let mut event_rx = events.subscribe();
        let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                Ok(event) = event_rx.recv() => {
                    if matches!(&event, AppEvent::WorkflowRunChanged { .. }) {
                        if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 100).await.is_err() {
                            return;
                        }
                    }
                }
                _ = poll_interval.tick() => {
                    if send_workflow_node_run_chunks(db.as_ref(), &mut tx, node_run_id, &mut cursor, 100).await.is_err() {
                        return;
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => return,
                        _ => {}
                    }
                }
            }
        }
    })
}

async fn ws_run_stream<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx_ws) = socket.split();
        let mut cursor: Option<i64> = None;
        if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 500)
            .await
            .is_err()
        {
            return;
        }
        let mut event_rx = events.subscribe();
        let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                Ok(event) = event_rx.recv() => {
                    let is_chunk = matches!(&event, AppEvent::RunChunkAdded { run_id: id } if *id == run_id);
                    let is_done = matches!(&event, AppEvent::RunStatusChanged { run_id: id } if *id == run_id);
                    if is_chunk || is_done {
                        if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 100).await.is_err() {
                            return;
                        }
                        if is_done {
                            break;
                        }
                    }
                }
                _ = poll_interval.tick() => {
                    if send_run_chunks(db.as_ref(), &mut tx, run_id, &mut cursor, 100).await.is_err() {
                        return;
                    }
                }
                msg = rx_ws.next() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => return,
                        _ => {}
                    }
                }
            }
        }
    })
}

pub fn build_router<T: DatabaseImpl>(pool: Arc<T>, events: EventSender) -> Router {
    Router::new()
        .route("/ws/events", get(ws_events))
        .route("/ws/workflow-runs/{id}", get(ws_workflow_run::<T>))
        .route("/ws/run-stream/{id}", get(ws_run_stream::<T>))
        .route(
            "/ws/workflow-node-runs/{id}/stream",
            get(ws_workflow_node_run_stream::<T>),
        )
        .route(
            "/workflows",
            get(get_workflows::<T>)
                .post(upsert_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}",
            get(get_workflow::<T>)
                .patch(upsert_workflow::<T>)
                .delete(delete_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/triggers",
            get(get_workflow_triggers::<T>)
                .post(upsert_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/due",
            get(get_due_workflow_triggers::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}",
            get(get_workflow_trigger::<T>)
                .patch(update_workflow_trigger::<T>)
                .delete(delete_workflow_trigger::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_triggers/{id}/runs",
            post(create_workflow_trigger_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs",
            get(get_workflow_runs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/{id}/runs",
            post(create_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}",
            get(get_workflow_run::<T>)
                .patch(update_workflow_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/step",
            post(step_debug_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/continue",
            post(continue_debug_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug",
            patch(update_workflow_run_debug::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/cancel",
            post(cancel_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/run_to_cursor",
            post(run_to_cursor_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/skip",
            post(skip_debug_workflow_node::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/debug/rerun_node",
            post(rerun_debug_workflow_node::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/{id}/replay",
            post(replay_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route("/supervisor/status", get(get_supervisor_status))
        .route(
            "/workflow_runs/{id}/nodes",
            post(create_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}",
            patch(update_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/chunks",
            get(get_workflow_node_run_chunks::<T>)
                .post(append_workflow_node_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}/artifacts",
            get(get_workflow_node_run_artifacts::<T>)
                .post(add_workflow_node_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/catalog/items",
            get(get_catalog_items::<T>)
                .post(upsert_catalog_item::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/external_items",
            get(get_external_items::<T>)
                .post(create_external_item::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/external_resources",
            get(get_external_resources::<T>)
                .post(create_external_resource::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/feedback",
            get(get_feedback::<T>)
                .post(create_feedback::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/gates",
            get(get_gates::<T>)
                .post(create_gate::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workspaces",
            get(get_workspaces::<T>)
                .post(create_workspace::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/change_sets",
            get(get_change_sets::<T>)
                .post(create_change_set::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/automation_events",
            get(get_automation_events::<T>)
                .post(create_automation_event::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/approvals",
            get(get_approvals::<T>)
                .post(create_approval::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/approvals/{id}/approve",
            post(approve_request::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/approvals/{id}/reject",
            post(reject_request::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/idempotency_keys",
            get(get_idempotency_key::<T>)
                .post(put_idempotency_key::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/credentials",
            get(get_credential)
                .post(put_credential)
                .delete(delete_credential),
        )
        .route(
            "/providers",
            get(get_providers::<T>)
                .post(upsert_provider::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/webhooks/wake",
            post(webhook_wake::<T>).layer(Extension(pool.clone())),
        )
        .layer(Extension(events))
}

pub async fn run_webserver<T: DatabaseImpl>(
    pool: Arc<T>,
    notify: Arc<Notify>,
    port: u16,
) -> Result<(), SendableError> {
    initialize_database(&pool).await?;
    seed_builtin_catalog(pool.as_ref()).await?;
    let (events_tx, _) = broadcast::channel::<AppEvent>(256);
    let app = build_router(pool, events_tx);
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(listener, app);
    info!("Webserver started at {}:{}", addr.ip(), addr.port());

    tokio::select! {
        result = server => {
            if let Err(err) = result {
                log::error!("Webserver error: {}", err);
                return Err(Box::new(err));
            }
            Ok(())
        }
        _ = notify.notified() => {
            info!("Shutting down web server...");
            Ok(())
        }
    }
}

async fn seed_builtin_catalog<T: DatabaseImpl>(db: &T) -> Result<(), SendableError> {
    for raw in [include_str!("../../packs/sdlc/workflow-pack.json")] {
        let item: serde_json::Value = serde_json::from_str(raw)?;
        db.upsert_catalog_item(item).await?;
    }
    Ok(())
}
