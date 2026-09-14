use axum::{
    Extension, Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use runinator_models::{
    auth::AuthContext,
    local_runtime::{LocalRuntimeComponentSnapshot, LocalRuntimeHostKind, LocalRuntimeSnapshot},
    rbac::{Action, ScopeRef},
};
use runinator_platform::app_data;
use runinator_ws_middleware::authz::AuthContextExt;

use crate::handlers::runs::compute_stale_seconds;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint};

pub async fn get_supervisor_status(Extension(ctx): Extension<AuthContext>) -> Response {
    if let Err(reply) = ctx.require_scope_action(Action::View, ScopeRef::PLATFORM) {
        return reply.into_reply().into_response();
    }
    let path = std::env::var("RUNINATOR_SUPERVISOR_STATE_PATH").unwrap_or_else(|_| {
        app_data::default_supervisor_state_dir()
            .map(|path| path.join("state.json").to_string_lossy().into_owned())
            .unwrap_or_else(|_| "supervisor/state.json".to_string())
    });
    let path_buf = std::path::PathBuf::from(&path);
    if !path_buf.exists() {
        // An absent local supervisor is an expected deployment mode, not an unavailable API
        // resource. Reply successfully so Command Center can disable its optional status poll
        // without leaving a benign 404 in the browser console.
        return (
            StatusCode::OK,
            Json(serde_json::json!({
                "configured": false,
                "path": path
            })),
        )
            .into_response();
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
            (StatusCode::OK, Json(body)).into_response()
        }
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "configured": true,
                "error": err.to_string()
            })),
        )
            .into_response(),
    }
}

pub async fn get_local_runtime_status(Extension(ctx): Extension<AuthContext>) -> Response {
    if let Err(reply) = ctx.require_scope_action(Action::View, ScopeRef::PLATFORM) {
        return reply.into_reply().into_response();
    }
    let standalone_path = std::env::var("RUNINATOR_STANDALONE_STATE_PATH").unwrap_or_else(|_| {
        app_data::app_data_path("standalone/state.json")
            .map(|path| path.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "standalone/state.json".to_string())
    });
    if let Ok(data) = std::fs::read_to_string(&standalone_path)
        && let Ok(snapshot) = serde_json::from_str::<LocalRuntimeSnapshot>(&data)
    {
        return local_runtime_reply(snapshot);
    }

    let supervisor_path = std::env::var("RUNINATOR_SUPERVISOR_STATE_PATH").unwrap_or_else(|_| {
        app_data::default_supervisor_state_dir()
            .map(|path| path.join("state.json").to_string_lossy().into_owned())
            .unwrap_or_else(|_| "supervisor/state.json".to_string())
    });
    if let Ok(snapshot) =
        runinator_supervisor::snapshot::read_snapshot(std::path::Path::new(&supervisor_path))
    {
        return local_runtime_reply(LocalRuntimeSnapshot {
            host_kind: LocalRuntimeHostKind::Supervisor,
            pid: snapshot.supervisor_pid,
            started_at: snapshot.started_at,
            updated_at: snapshot.updated_at,
            components: snapshot
                .processes
                .into_iter()
                .map(|process| LocalRuntimeComponentSnapshot {
                    id: process.name,
                    kind: "process".into(),
                    status: process.status,
                    restarts: process.restarts,
                    uptime_seconds: process.uptime_seconds,
                    last_error: process.last_error,
                })
                .collect(),
        });
    }

    (
        StatusCode::OK,
        Json(serde_json::json!({ "configured": false })),
    )
        .into_response()
}

fn local_runtime_reply(snapshot: LocalRuntimeSnapshot) -> Response {
    let stale_seconds = compute_stale_seconds(&snapshot.updated_at);
    let mut body = serde_json::to_value(snapshot).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(object) = body.as_object_mut() {
        object.insert("configured".into(), serde_json::json!(true));
        object.insert("stale_seconds".into(), serde_json::json!(stale_seconds));
    }
    (StatusCode::OK, Json(body)).into_response()
}

/// local runtime-host status endpoints.
pub fn routes() -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route("/supervisor/status", get(get_supervisor_status))
        .route("/local-runtime/status", get(get_local_runtime_status))
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
        "get",
        "/supervisor/status",
        "Local runtime",
        "Get local supervisor status",
        "Returns status for the local supervisor stack when the web service is running under it.",
        false,
        None,
        &[],
        200,
        "supervisor status",
        Example::Supervisor,
    ),
    endpoint!(
        "get",
        "/local-runtime/status",
        "Local runtime",
        "Get local runtime host status",
        "Returns logical component status for a standalone or supervisor-hosted local stack.",
        false,
        None,
        &[],
        200,
        "local runtime status",
        Example::Supervisor,
    ),
];
