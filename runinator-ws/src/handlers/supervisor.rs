use axum::{Json, http::StatusCode};
use runinator_utilities::app_data;

use crate::handlers::runs::compute_stale_seconds;

pub(crate) async fn get_supervisor_status() -> (StatusCode, Json<serde_json::Value>) {
    let path = std::env::var("RUNINATOR_SUPERVISOR_STATE_PATH").unwrap_or_else(|_| {
        app_data::default_supervisor_state_dir()
            .map(|path| path.join("state.json").to_string_lossy().into_owned())
            .unwrap_or_else(|_| "supervisor/state.json".to_string())
    });
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
            let mut body =
                serde_json::to_value(&snapshot).unwrap_or_else(|_| serde_json::json!({}));
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
