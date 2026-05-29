use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::value::Value;
use runinator_models::{
    bundles::ProviderBundle,
    providers::{ProviderMetadata, validate_provider_metadata},
};

use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request};

pub(crate) async fn get_providers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
) -> (StatusCode, Json<ApiResponse>) {
    let items = match repository::fetch_catalog_items(db.as_ref(), Some("provider_metadata".into()))
        .await
    {
        Ok(items) => items,
        Err(err) => return api_error(err.to_string()),
    };

    match provider_metadata_from_items(items) {
        Ok(providers) => (StatusCode::OK, Json(ApiResponse::ProviderList(providers))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn upsert_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(provider): Json<ProviderMetadata>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(err) = validate_provider_metadata(&provider) {
        return bad_request(err);
    }
    let item = provider_catalog_item(&provider);
    let item = match repository::upsert_catalog_item(db.as_ref(), item).await {
        Ok(item) => item,
        Err(err) => return api_error(err.to_string()),
    };

    match provider_metadata_from_item(item) {
        Ok(provider) => (StatusCode::OK, Json(ApiResponse::Provider(provider))),
        Err(err) => api_error(err.to_string()),
    }
}

pub(crate) async fn import_provider_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Json(bundle): Json<ProviderBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    let mut imported = Vec::with_capacity(bundle.providers.len());
    for provider in &bundle.providers {
        if let Err(err) = validate_provider_metadata(provider) {
            return bad_request(err);
        }
        let item = provider_catalog_item(provider);
        let item = match repository::upsert_catalog_item(db.as_ref(), item).await {
            Ok(item) => item,
            Err(err) => return api_error(err.to_string()),
        };
        match provider_metadata_from_item(item) {
            Ok(provider) => imported.push(provider),
            Err(err) => return api_error(err.to_string()),
        }
    }
    (
        StatusCode::OK,
        Json(ApiResponse::ProviderBundle(ProviderBundle {
            providers: imported,
        })),
    )
}

pub(crate) fn provider_metadata_from_items(
    items: Vec<Value>,
) -> Result<Vec<ProviderMetadata>, serde_json::Error> {
    let mut providers = items
        .into_iter()
        .map(provider_metadata_from_item)
        .collect::<Result<Vec<_>, _>>()?;
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(providers)
}

pub(crate) fn provider_metadata_from_item(
    item: Value,
) -> Result<ProviderMetadata, serde_json::Error> {
    let document = item.get("document").cloned().unwrap_or(item);
    serde_json::from_value(document.into())
}

pub(crate) fn provider_catalog_item(provider: &ProviderMetadata) -> Value {
    runinator_models::json!({
        "uri": format!("runinator://providers/{}", provider.name),
        "item_type": "provider_metadata",
        "name": provider.name,
        "version": "1",
        "document": provider,
        "metadata": {}
    })
}
