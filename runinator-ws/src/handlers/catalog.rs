use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use runinator_models::errors::SendableError;
use runinator_models::json;
use runinator_models::value::Value;

use crate::models::{ApiResponse, CatalogQuery};
use crate::repository;
use crate::responses::{api_error, not_found};

pub(crate) async fn get_catalog_items<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(_ctx): Extension<AuthContext>,
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

pub(crate) async fn upsert_catalog_item<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(item): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_admin(&ctx) {
        return reply;
    }
    match repository::upsert_catalog_item(db.as_ref(), item).await {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn seed_builtin_catalog<T: DatabaseImpl>(db: &T) -> Result<(), SendableError> {
    for raw in [include_str!("../../../packs/sdlc/sdlc.wdlp")] {
        let item = wdl_pack_catalog_item(raw)?;
        db.upsert_catalog_item(item).await?;
    }
    Ok(())
}

fn wdl_pack_catalog_item(raw: &str) -> Result<Value, SendableError> {
    let manifest: Value = serde_json::from_str(raw)?;
    let version = manifest
        .get("version")
        .and_then(|value| {
            value
                .as_str()
                .map(str::to_string)
                .or_else(|| value.as_i64().map(|number| number.to_string()))
        })
        .unwrap_or_else(|| "1".to_string());
    Ok(json!({
        "uri": "runinator://packs/sdlc",
        "item_type": "wdl_pack",
        "name": manifest.get("name").and_then(Value::as_str).unwrap_or("SDLC Automation Pack"),
        "version": version,
        "document": {
            "workflows": manifest.get("workflows").cloned().unwrap_or_else(|| json!([])),
            "triggers": manifest.get("triggers").cloned().unwrap_or_else(|| json!([]))
        }
    }))
}
