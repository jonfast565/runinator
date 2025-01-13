mod repository;

use axum::{
    routing::{delete, get, patch, post},
    Extension, Json, Router,
};
use log::info;
use runinator_database::sqlite::SqliteDb;
use runinator_models::{
    core::{ScheduledTask, TaskRun},
    web::TaskResponse,
};
use std::sync::Arc;
use tokio::sync::Notify;

async fn add_task(
    Extension(db): Extension<Arc<SqliteDb>>,
    Json(task_input): Json<ScheduledTask>,
) -> Json<TaskResponse> {
    let r = repository::add_task(db.as_ref(), &task_input).await;
    match r {
        Ok(r) => Json(r),
        Err(_) => panic!("Failed"),
    }
}

async fn update_task(
    Extension(db): Extension<Arc<SqliteDb>>,
    Json(task_input): Json<ScheduledTask>,
) -> Json<TaskResponse> {
    info!("Updating task: {:?}", task_input);
    let r = repository::update_task(db.as_ref(), &task_input).await;
    match r {
        Ok(r) => Json(r),
        Err(_) => panic!("Failed"),
    }
}

async fn delete_task(
    Extension(db): Extension<Arc<SqliteDb>>,
    axum::extract::Path(task_id): axum::extract::Path<i64>,
) -> Json<TaskResponse> {
    info!("Deleting task with ID: {}", task_id);
    let r = repository::delete_task(db.as_ref(), task_id).await;
    match r {
        Ok(r) => Json(r),
        Err(_) => panic!("Failed"),
    }
}

async fn get_tasks(
    Extension(db): Extension<Arc<SqliteDb>>,
    //axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Vec<ScheduledTask>> {
    println!("Fetching all tasks");
    let r = repository::fetch_tasks(db.as_ref()).await;
    match r {
        Ok(r) => Json(r),
        Err(_) => panic!("Failed"),
    }
}

async fn get_task_runs(
    Extension(db): Extension<Arc<SqliteDb>>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Json<Vec<TaskRun>> {
    let start_time = params
        .get("start_time")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);

    let end_time = params
        .get("end_time")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(i64::MAX);

    println!("Fetching task runs between {} and {}", start_time, end_time);
    let result = repository::fetch_task_runs(db.as_ref(), start_time, end_time).await;
    match result {
        Ok(r) => Json(r),
        Err(_) => panic!("Failed"),
    }
}

pub async fn run_webserver(pool: &Arc<SqliteDb>, notify: Arc<Notify>, port: u16) {
    let app = Router::new()
        .route("/tasks", get(get_tasks).layer(Extension(pool.clone())))
        .route("/tasks", post(add_task).layer(Extension(pool.clone())))
        .route("/tasks", patch(update_task).layer(Extension(pool.clone())))
        .route(
            "/tasks/:id",
            delete(delete_task).layer(Extension(pool.clone())),
        )
        .route(
            "/task_runs",
            get(get_task_runs).layer(Extension(pool.clone())),
        );

    let addr = format!("0.0.0.0:{}", port).parse().unwrap();
    let server = axum::Server::bind(&addr).serve(app.into_make_service());

    tokio::select! {
        result = server => {
            if let Err(err) = result {
                eprintln!("Webserver error: {}", err);
            }
        }
        _ = notify.notified() => {
            println!("Shutting down web server...");
        }
    }
}
