use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::errors::SendableError;
use runinator_models::value::Value;

use crate::models::{ApiResponse, CatalogQuery};
use crate::repository;
use crate::responses::{api_error, not_found};

pub(crate) async fn get_catalog_items<T: DatabaseImpl>(
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

pub(crate) async fn upsert_catalog_item<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(item): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    match repository::upsert_catalog_item(db.as_ref(), item).await {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn seed_builtin_catalog<T: DatabaseImpl>(db: &T) -> Result<(), SendableError> {
    for raw in [include_str!("../../../packs/sdlc/workflow-pack.json")] {
        let item: Value = serde_json::from_str(raw)?;
        db.upsert_catalog_item(item).await?;
    }
    Ok(())
}
