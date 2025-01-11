use axum::Router;
use runinator_database::interfaces::DatabaseImpl;
use std::sync::Arc;

pub async fn run_webserver(_pool: Arc<impl DatabaseImpl>, port: u16) {
    let app = Router::new();
        //.route("/tasks", post(add_task.layer(axum::extract::Extension(pool.clone()))))
        //.route("/tasks", patch(update_task.layer(axum::extract::Extension(pool.clone()))))
        //.route("/tasks/:id", delete(delete_task.layer(axum::extract::Extension(pool.clone()))))
        //.route("/task_runs", get(get_task_runs.layer(axum::extract::Extension(pool.clone()))));

    axum::Server::bind(&format!("0.0.0.0:{}", port).parse().unwrap())
        .serve(app.into_make_service())
        .await
        .unwrap();
}
