mod models;
mod repository;
mod discovery;
mod config;

use std::{collections::HashMap, net::SocketAddr, sync::Arc};

use axum::{
    Extension, Json, Router,
    extract::{Path, Query},
    http::StatusCode,
    routing::{delete, get, patch, post},
};
use log::info;
use models::{ApiError, ApiResponse, TaskRunRequest};
use runinator_database::{initialize_database, interfaces::DatabaseImpl};
use runinator_models::{core::ScheduledTask, errors::SendableError, web::TaskResponse};
use tokio::sync::Notify;

async fn add_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(task_input): Json<ScheduledTask>,
) -> (StatusCode, Json<ApiResponse>) {
    let r = repository::add_task(db.as_ref(), &task_input).await;
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

async fn update_task<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Path(task_id): Path<i64>,
    Json(mut task_input): Json<ScheduledTask>,
) -> (StatusCode, Json<ApiResponse>) {
    if task_input.id != Some(task_id) {
        task_input.id = Some(task_id);
    }
    info!("Updating task: {:?}", task_input);
    let r = repository::update_task(db.as_ref(), &task_input).await;
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
            post(request_run::<T>).layer(Extension(pool)),
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
