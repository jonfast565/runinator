use axum::{Json, http::StatusCode};
use runinator_models::value::Value;
use runinator_workflows::{enum_catalogs, node_kind_catalog, trigger_kind_catalog};

use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::responses::api_error;

// these catalogs are compile-time constants, so they are served straight from the workflows crate
// rather than persisted; there is no per-replica or per-tenant variation to store.

fn json_response(
    value: Result<serde_json::Value, serde_json::Error>,
) -> (StatusCode, Json<ApiResponse>) {
    match value {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(Value::from(value))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

/// UI metadata for every workflow node kind (palette, generic editor, detail view, edge slots).
#[utoipa::path(
    get,
    path = "/node-kinds",
    tag = "Catalog",
    responses((status = 200, description = "workflow node kind metadata", body = serde_json::Value)),
)]
pub async fn get_node_kinds() -> (StatusCode, Json<ApiResponse>) {
    json_response(serde_json::to_value(node_kind_catalog()))
}

/// UI metadata for every workflow trigger kind (config forms).
#[utoipa::path(
    get,
    path = "/trigger-kinds",
    tag = "Catalog",
    responses((status = 200, description = "workflow trigger kind metadata", body = serde_json::Value)),
)]
pub async fn get_trigger_kinds() -> (StatusCode, Json<ApiResponse>) {
    json_response(serde_json::to_value(trigger_kind_catalog()))
}

/// small closed enums the frontend renders as select controls (gate/match/branch-policy/setting,
/// interrupt-source/resume-mode/concurrency-policy).
#[utoipa::path(
    get,
    path = "/catalog/enums",
    tag = "Catalog",
    responses((status = 200, description = "closed enum metadata", body = serde_json::Value)),
)]
pub async fn get_enum_catalogs() -> (StatusCode, Json<ApiResponse>) {
    json_response(serde_json::to_value(enum_catalogs()))
}

/// the `catalog_metadata` endpoints.
pub fn routes() -> axum::Router {
    use axum::routing::get;
    axum::Router::new()
        .route("/node-kinds", get(get_node_kinds))
        .route("/trigger-kinds", get(get_trigger_kinds))
        .route("/catalog/enums", get(get_enum_catalogs))
}
