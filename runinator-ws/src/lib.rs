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
    routing::{delete, get, patch, post},
};
use futures::{SinkExt, StreamExt};
use log::info;
use models::{
    ApiError, ApiResponse, ApprovalResolutionRequest, AutomationRecordQuery, CatalogQuery,
    CredentialPutRequest, CredentialQuery, IdempotencyRequest, RunStatusQuery, RunStatusRequest,
    TaskRunRequest, WebhookWakeRequest, WorkflowNodeRunRequest, WorkflowNodeRunStatusRequest,
    WorkflowRunRequest, WorkflowRunStatusQuery, WorkflowRunStatusRequest,
};
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::{
    core::ScheduledTask,
    errors::SendableError,
    providers::ProviderMetadata,
    runs::{NewRunArtifact, NewRunChunk, RunRequest},
    web::TaskResponse,
    workflows::WorkflowDefinition,
};
use runinator_utilities::credential_store::{
    CredentialStore, LocalEncryptedCredentialStore, default_credential_store_path,
};
use serde::{Deserialize, Serialize};
use tokio::{net::TcpListener, sync::{broadcast, Notify}};

#[derive(Clone, Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AppEvent {
    TasksChanged,
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

#[derive(Debug, Default, Deserialize)]
struct TaskMutationParams {
    #[serde(default)]
    override_next_execution: bool,
}

#[derive(Debug, Default, Deserialize)]
struct ChunkQuery {
    cursor: Option<i64>,
    limit: Option<i64>,
}

impl TaskMutationParams {
    fn should_override(&self) -> bool {
        self.override_next_execution
    }
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

async fn preserve_next_execution_if_needed<T: DatabaseImpl>(
    db: &T,
    task: &mut ScheduledTask,
    override_next_execution: bool,
) -> Result<(), SendableError> {
    if override_next_execution {
        return Ok(());
    }

    let Some(task_id) = task.id else {
        return Ok(());
    };

    if let Some(existing) = db.fetch_task_by_id(task_id).await? {
        task.next_execution = existing.next_execution;
    }

    Ok(())
}

async fn add_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Query(params): Query<TaskMutationParams>,
    Json(mut task_input): Json<ScheduledTask>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(err) =
        preserve_next_execution_if_needed(db.as_ref(), &mut task_input, params.should_override())
            .await
    {
        return api_error(err.to_string());
    }

    match repository::add_task(db.as_ref(), &task_input).await {
        Ok(r) => {
            emit(&events, AppEvent::TasksChanged);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(r)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(task_id): Path<i64>,
    Query(params): Query<TaskMutationParams>,
    Json(mut task_input): Json<ScheduledTask>,
) -> (StatusCode, Json<ApiResponse>) {
    if task_input.id != Some(task_id) {
        task_input.id = Some(task_id);
    }
    info!("Updating task: {:?}", task_input);

    if let Err(err) =
        preserve_next_execution_if_needed(db.as_ref(), &mut task_input, params.should_override())
            .await
    {
        return api_error(err.to_string());
    }

    match repository::update_task(db.as_ref(), &task_input).await {
        Ok(r) => {
            emit(&events, AppEvent::TasksChanged);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(r)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn delete_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(task_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    info!("Deleting task with ID: {}", task_id);
    let r = repository::delete_task(db.as_ref(), task_id).await;
    match r {
        Ok(r) => {
            emit(&events, AppEvent::TasksChanged);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(r)))
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::ApiError(ApiError {
                message: err.to_string(),
            })),
        ),
    }
}

async fn get_tasks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    info!("Fetching all tasks");
    let r = repository::fetch_tasks(db.as_ref()).await;
    match r {
        Ok(r) => (StatusCode::OK, Json(ApiResponse::ScheduledTaskList(r))),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::ApiError(ApiError {
                message: err.to_string(),
            })),
        ),
    }
}

async fn get_task_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(params): Query<HashMap<String, String>>,
) -> (StatusCode, Json<ApiResponse>) {
    let start_time = params
        .get("start_time")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);

    let end_time = params
        .get("end_time")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    info!("Fetching task runs between {} and {}", start_time, end_time);
    let result = repository::fetch_task_runs(db.as_ref(), start_time, end_time).await;
    match result {
        Ok(r) => (StatusCode::OK, Json(ApiResponse::ScheduleTaskRuns(r))),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::ApiError(ApiError {
                message: err.to_string(),
            })),
        ),
    }
}

async fn request_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(task_id): Path<i64>,
) -> (StatusCode, Json<TaskResponse>) {
    info!("Requesting run of task {}", task_id);
    match repository::request_run(db.as_ref(), task_id).await {
        Ok(resp) => (StatusCode::OK, Json(resp)),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(TaskResponse {
                success: false,
                message: err.to_string(),
            }),
        ),
    }
}

async fn create_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(task_id): Path<i64>,
    Json(request): Json<RunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_run(db.as_ref(), task_id, &request).await {
        Ok(run) => (StatusCode::ACCEPTED, Json(ApiResponse::RunSummary(run))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_task_runs_v2<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(task_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_runs_for_task(db.as_ref(), task_id).await {
        Ok(runs) => (StatusCode::OK, Json(ApiResponse::RunList(runs))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<RunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match query.status {
        Some(status) => match repository::fetch_runs_by_status(db.as_ref(), status).await {
            Ok(runs) => (StatusCode::OK, Json(ApiResponse::RunList(runs))),
            Err(err) => api_error(err.to_string()),
        },
        None => api_error("runs query requires status"),
    }
}

async fn get_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_run(db.as_ref(), run_id).await {
        Ok(Some(run)) => (StatusCode::OK, Json(ApiResponse::RunSummary(run))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::ApiError(ApiError {
                message: format!("Run {run_id} not found"),
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    Json(request): Json<RunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_run_status(
        db.as_ref(),
        run_id,
        request.status,
        request.output_json,
        request.message,
    )
    .await
    {
        Ok(resp) => {
            emit(&events, AppEvent::RunStatusChanged { run_id });
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_run_chunks<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
    Query(query): Query<ChunkQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_run_chunks(
        db.as_ref(),
        run_id,
        query.cursor,
        query.limit.unwrap_or(100),
    )
    .await
    {
        Ok(chunks) => (StatusCode::OK, Json(ApiResponse::RunChunks(chunks))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn append_run_chunk<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::append_run_chunk(db.as_ref(), run_id, &chunk).await {
        Ok(resp) => {
            emit(&events, AppEvent::RunChunkAdded { run_id });
            (StatusCode::ACCEPTED, Json(ApiResponse::TaskResponse(resp)))
        }
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_run_artifacts<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_run_artifacts(db.as_ref(), run_id).await {
        Ok(artifacts) => (StatusCode::OK, Json(ApiResponse::RunArtifacts(artifacts))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn add_run_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(run_id): Path<i64>,
    Json(artifact): Json<NewRunArtifact>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::add_run_artifact(db.as_ref(), run_id, &artifact).await {
        Ok(resp) => (StatusCode::ACCEPTED, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_artifact<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(artifact_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_artifact(db.as_ref(), artifact_id).await {
        Ok(Some(artifact)) => (StatusCode::OK, Json(ApiResponse::RunArtifact(artifact))),
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::ApiError(ApiError {
                message: format!("Artifact {artifact_id} not found"),
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
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
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::ApiError(ApiError {
                message: format!("Workflow {workflow_id} not found"),
            })),
        ),
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

async fn create_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_id): Path<i64>,
    Json(request): Json<WorkflowRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_run(db.as_ref(), workflow_id, request.parameters).await {
        Ok(run) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                run,
                nodes: Vec::new(),
            })),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_runs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Query(query): Query<WorkflowRunStatusQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    match (query.workflow_id, query.status) {
        (Some(workflow_id), _) => {
            match repository::fetch_workflow_runs_for_workflow(db.as_ref(), workflow_id).await {
                Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
                Err(err) => api_error(err.to_string()),
            }
        }
        (None, Some(status)) => {
            match repository::fetch_workflow_runs_by_status(db.as_ref(), status).await {
                Ok(runs) => (StatusCode::OK, Json(ApiResponse::WorkflowRunList(runs))),
                Err(err) => api_error(err.to_string()),
            }
        }
        (None, None) => api_error("workflow_runs query requires workflow_id or status"),
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
            emit(&events, AppEvent::WorkflowRunChanged { run_id: workflow_run_id });
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
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(ApiResponse::ApiError(ApiError {
                message: format!("Workflow run {workflow_run_id} not found"),
            })),
        ),
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
            emit(&events, AppEvent::WorkflowRunChanged { run_id: workflow_run_id });
            (StatusCode::ACCEPTED, Json(ApiResponse::WorkflowNodeRun(step)))
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
        request.task_run_id,
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
            emit(&events, AppEvent::WorkflowRunActivity);
            (StatusCode::OK, Json(ApiResponse::TaskResponse(resp)))
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
    let (scope, name) = match (query.scope, query.name) {
        (Some(scope), Some(name)) => (scope, name),
        (None, None) => {
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
        _ => return bad_request("credential lookup requires both scope and name"),
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
    match repository::fetch_catalog_items(db.as_ref(), Some("provider_metadata".into())).await {
        Ok(items) => match provider_metadata_from_items(items) {
            Ok(providers) => (StatusCode::OK, Json(ApiResponse::ProviderList(providers))),
            Err(err) => api_error(err.to_string()),
        },
        Err(err) => api_error(err.to_string()),
    }
}

async fn upsert_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(provider): Json<ProviderMetadata>,
) -> (StatusCode, Json<ApiResponse>) {
    let item = provider_catalog_item(&provider);
    match repository::upsert_catalog_item(db.as_ref(), item).await {
        Ok(item) => match provider_metadata_from_item(item) {
            Ok(provider) => (StatusCode::OK, Json(ApiResponse::Provider(provider))),
            Err(err) => api_error(err.to_string()),
        },
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
    Json(request): Json<WebhookWakeRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let Ok(Some((run, node_runs))) =
        repository::fetch_workflow_run(db.as_ref(), request.workflow_run_id).await
    else {
        return not_found(format!(
            "Workflow run {} not found",
            request.workflow_run_id
        ));
    };
    let node_id = request
        .node_id
        .clone()
        .or(run.active_node_id)
        .unwrap_or_default();
    if let Some(node_run) = node_runs
        .iter()
        .filter(|node_run| node_run.node_id == node_id)
        .max_by_key(|node_run| node_run.id)
    {
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
            None,
            Some(state.clone()),
            Some("webhook_wake".into()),
            request.message.clone(),
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
    }
    (
        StatusCode::OK,
        Json(ApiResponse::TaskResponse(TaskResponse {
            success: true,
            message: "Webhook wake recorded".into(),
        })),
    )
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

async fn record_task_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(payload): Json<TaskRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    let r = repository::log_task_run(db.as_ref(), &payload).await;
    match r {
        Ok(r) => (StatusCode::ACCEPTED, Json(ApiResponse::TaskResponse(r))),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ApiResponse::ApiError(ApiError {
                message: err.to_string(),
            })),
        ),
    }
}

async fn ws_events(
    Extension(events): Extension<EventSender>,
    ws: WebSocketUpgrade,
) -> Response {
    let mut rx = events.subscribe();
    ws.on_upgrade(move |socket| async move {
        let (mut tx, _rx) = socket.split();
        while let Ok(event) = rx.recv().await {
            if let Ok(payload) = serde_json::to_string(&event) {
                if tx.send(Message::Text(payload.into())).await.is_err() {
                    break;
                }
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
        // Send initial state
        if let Ok(Some((run, nodes))) = repository::fetch_workflow_run(db.as_ref(), run_id).await {
            if let Ok(payload) = serde_json::to_string(&models::WorkflowRunResponse { run, nodes }) {
                let _ = tx.send(Message::Text(payload.into())).await;
            }
        }
        let mut event_rx = events.subscribe();
        loop {
            tokio::select! {
                Ok(event) = event_rx.recv() => {
                    let relevant = matches!(&event,
                        AppEvent::WorkflowRunChanged { run_id: id } if *id == run_id
                    ) || matches!(&event, AppEvent::WorkflowRunActivity);
                    if relevant {
                        match repository::fetch_workflow_run(db.as_ref(), run_id).await {
                            Ok(Some((run, nodes))) => {
                                let terminal = run.status.is_terminal();
                                if let Ok(payload) = serde_json::to_string(&models::WorkflowRunResponse { run, nodes }) {
                                    if tx.send(Message::Text(payload.into())).await.is_err() {
                                        break;
                                    }
                                }
                                if terminal { break; }
                            }
                            _ => break,
                        }
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

async fn ws_run_stream<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Path(run_id): Path<i64>,
    ws: WebSocketUpgrade,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        let (mut tx, mut rx_ws) = socket.split();
        let mut cursor: Option<i64> = None;
        // Send any existing chunks first
        if let Ok(chunks) = repository::fetch_run_chunks(db.as_ref(), run_id, cursor, 500).await {
            for chunk in &chunks {
                if let Ok(payload) = serde_json::to_string(chunk) {
                    if tx.send(Message::Text(payload.into())).await.is_err() {
                        return;
                    }
                }
                cursor = Some(chunk.sequence);
            }
        }
        let mut event_rx = events.subscribe();
        let mut poll_interval = tokio::time::interval(Duration::from_millis(500));
        loop {
            tokio::select! {
                Ok(event) = event_rx.recv() => {
                    let is_chunk = matches!(&event, AppEvent::RunChunkAdded { run_id: id } if *id == run_id);
                    let is_done = matches!(&event, AppEvent::RunStatusChanged { run_id: id } if *id == run_id);
                    if is_chunk || is_done {
                        if let Ok(chunks) = repository::fetch_run_chunks(db.as_ref(), run_id, cursor, 100).await {
                            for chunk in &chunks {
                                if let Ok(payload) = serde_json::to_string(chunk) {
                                    if tx.send(Message::Text(payload.into())).await.is_err() {
                                        return;
                                    }
                                }
                                cursor = Some(chunk.sequence);
                            }
                        }
                        if is_done { break; }
                    }
                }
                _ = poll_interval.tick() => {
                    // Fallback poll in case events are missed
                    if let Ok(chunks) = repository::fetch_run_chunks(db.as_ref(), run_id, cursor, 100).await {
                        for chunk in &chunks {
                            if let Ok(payload) = serde_json::to_string(chunk) {
                                if tx.send(Message::Text(payload.into())).await.is_err() {
                                    return;
                                }
                            }
                            cursor = Some(chunk.sequence);
                        }
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
        .route("/ws/workflow-runs/:id", get(ws_workflow_run::<T>))
        .route("/ws/runs/:id/stream", get(ws_run_stream::<T>))
        .route("/tasks", get(get_tasks::<T>).layer(Extension(pool.clone())))
        .route("/tasks", post(add_task::<T>).layer(Extension(pool.clone())))
        .route(
            "/tasks/{id}",
            patch(update_task::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/tasks/{id}",
            delete(delete_task::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/task_runs",
            get(get_task_runs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/task_runs",
            post(record_task_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/tasks/{id}/request_run",
            post(request_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/tasks/{id}/runs",
            post(create_run::<T>)
                .get(get_task_runs_v2::<T>)
                .layer(Extension(pool.clone())),
        )
        .route("/runs", get(get_runs::<T>).layer(Extension(pool.clone())))
        .route(
            "/runs/{id}",
            get(get_run::<T>)
                .patch(update_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/runs/{id}/chunks",
            get(get_run_chunks::<T>)
                .post(append_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/runs/{id}/artifacts",
            get(get_run_artifacts::<T>)
                .post(add_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/{id}",
            get(get_artifact::<T>).layer(Extension(pool.clone())),
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
            "/workflow_runs/{id}/nodes",
            post(create_workflow_node_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_node_runs/{id}",
            patch(update_workflow_node_run::<T>).layer(Extension(pool.clone())),
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
