mod config;
mod models;
mod repository;

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::{delete, get, patch, post},
};
use log::info;
use models::{
    ApiError, ApiResponse, RunStatusQuery, RunStatusRequest, TaskRunRequest, WorkflowRunRequest,
    WorkflowRunStatusQuery, WorkflowRunStatusRequest, WorkflowStepRunRequest,
    WorkflowStepRunStatusRequest,
};
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::{
    core::ScheduledTask,
    errors::SendableError,
    runs::{NewRunArtifact, NewRunChunk, RunRequest},
    web::TaskResponse,
    workflows::WorkflowDefinition,
};
use serde::Deserialize;
use tokio::sync::Notify;

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
        Ok(r) => (StatusCode::OK, Json(ApiResponse::TaskResponse(r))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
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
        Ok(r) => (StatusCode::OK, Json(ApiResponse::TaskResponse(r))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn delete_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(task_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    info!("Deleting task with ID: {}", task_id);
    let r = repository::delete_task(db.as_ref(), task_id).await;
    match r {
        Ok(r) => (StatusCode::OK, Json(ApiResponse::TaskResponse(r))),
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
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
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
    Path(run_id): Path<i64>,
    Json(chunk): Json<NewRunChunk>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::append_run_chunk(db.as_ref(), run_id, &chunk).await {
        Ok(resp) => (StatusCode::ACCEPTED, Json(ApiResponse::TaskResponse(resp))),
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
    Json(workflow): Json<WorkflowDefinition>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::upsert_workflow(db.as_ref(), &workflow).await {
        Ok(workflow) => (StatusCode::OK, Json(ApiResponse::Workflow(workflow))),
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
                steps: Vec::new(),
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
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<WorkflowRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_workflow_run_status(
        db.as_ref(),
        workflow_run_id,
        request.status,
        request.message,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
    }
}

async fn get_workflow_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_run_id): Path<i64>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::fetch_workflow_run(db.as_ref(), workflow_run_id).await {
        Ok(Some((run, steps))) => (
            StatusCode::OK,
            Json(ApiResponse::WorkflowRun(models::WorkflowRunResponse {
                run,
                steps,
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

async fn create_workflow_step_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(workflow_run_id): Path<i64>,
    Json(request): Json<WorkflowStepRunRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::create_workflow_step_run(
        db.as_ref(),
        workflow_run_id,
        request.step_id,
        request.parameters,
    )
    .await
    {
        Ok(step) => (
            StatusCode::ACCEPTED,
            Json(ApiResponse::WorkflowStepRun(step)),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

async fn update_workflow_step_run<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(step_run_id): Path<i64>,
    Json(request): Json<WorkflowStepRunStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::update_workflow_step_run(
        db.as_ref(),
        step_run_id,
        request.status,
        request.task_run_id,
        request.attempt,
        request.parameters,
        request.message,
    )
    .await
    {
        Ok(resp) => (StatusCode::OK, Json(ApiResponse::TaskResponse(resp))),
        Err(err) => api_error(err.to_string()),
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

pub fn build_router<T: DatabaseImpl>(pool: Arc<T>) -> Router {
    Router::new()
        .route("/tasks", get(get_tasks::<T>).layer(Extension(pool.clone())))
        .route("/tasks", post(add_task::<T>).layer(Extension(pool.clone())))
        .route(
            "/tasks/:id",
            patch(update_task::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/tasks/:id",
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
            "/tasks/:id/request_run",
            post(request_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/tasks/:id/runs",
            post(create_run::<T>)
                .get(get_task_runs_v2::<T>)
                .layer(Extension(pool.clone())),
        )
        .route("/runs", get(get_runs::<T>).layer(Extension(pool.clone())))
        .route(
            "/runs/:id",
            get(get_run::<T>)
                .patch(update_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/runs/:id/chunks",
            get(get_run_chunks::<T>)
                .post(append_run_chunk::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/runs/:id/artifacts",
            get(get_run_artifacts::<T>)
                .post(add_run_artifact::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/artifacts/:id",
            get(get_artifact::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflows",
            get(get_workflows::<T>)
                .post(upsert_workflow::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflows/:id",
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
            "/workflows/:id/runs",
            post(create_workflow_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/:id",
            get(get_workflow_run::<T>)
                .patch(update_workflow_run::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_runs/:id/steps",
            post(create_workflow_step_run::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/workflow_step_runs/:id",
            patch(update_workflow_step_run::<T>).layer(Extension(pool)),
        )
}

pub async fn run_webserver<T: DatabaseImpl>(
    pool: Arc<T>,
    notify: Arc<Notify>,
    port: u16,
) -> Result<(), SendableError> {
    initialize_database(&pool).await?;
    let app = build_router(pool);
    let addr: SocketAddr = format!("0.0.0.0:{port}").parse().unwrap();
    let server = axum::Server::bind(&addr).serve(app.into_make_service());
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
