use std::sync::Arc;

use axum::{Extension, Json, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::value::Value;
use runinator_models::{
    auth::AuthContext,
    bundles::ProviderBundle,
    providers::{ProviderMetadata, validate_provider_metadata},
};

use crate::repository;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{EndpointDoc, Example, endpoint, json_body};
use runinator_ws_core::responses::{api_error, bad_request};
use runinator_ws_middleware::authz::AuthContextExt;

/// list registered task providers and their action metadata.
#[utoipa::path(
    get,
    path = "/providers",
    tag = "Providers",
    responses((status = 200, description = "registered providers", body = serde_json::Value)),
)]
pub async fn get_providers<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(_ctx): Extension<AuthContext>,
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

pub async fn upsert_provider<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(provider): Json<ProviderMetadata>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_agent_service_or_admin() {
        return reply;
    }
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

pub async fn import_provider_bundle<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(bundle): Json<ProviderBundle>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_service_or_admin() {
        return reply;
    }
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

pub fn provider_metadata_from_items(
    items: Vec<Value>,
) -> Result<Vec<ProviderMetadata>, serde_json::Error> {
    let mut providers = items
        .into_iter()
        .map(provider_metadata_from_item)
        .collect::<Result<Vec<_>, _>>()?;
    providers.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(providers)
}

pub fn provider_metadata_from_item(item: Value) -> Result<ProviderMetadata, serde_json::Error> {
    let document = item.get("document").cloned().unwrap_or(item);
    serde_json::from_value(document.into())
}

pub fn provider_catalog_item(provider: &ProviderMetadata) -> Value {
    runinator_models::json!({
        "uri": format!("runinator://providers/{}", provider.name),
        "item_type": "provider_metadata",
        "name": provider.name,
        "version": "1",
        "document": provider,
        "metadata": {}
    })
}

/// the `providers` endpoints.
pub fn routes<T: DatabaseImpl>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_PROVIDERS,
            get(get_providers::<T>)
                .post(upsert_provider::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/providers/import",
            post(import_provider_bundle::<T>).layer(Extension(pool.clone())),
        )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint(
        "get",
        "/providers",
        "Providers",
        "List providers",
        "Lists registered provider metadata used by workers and workflow authoring.",
        false,
        None,
        &[],
        200,
        "providers",
        Example::ProviderList,
    ),
    endpoint(
        "post",
        "/providers",
        "Providers",
        "Upsert a provider",
        "Stores provider metadata for a provider implementation.",
        false,
        json_body("Provider metadata.", Example::Provider),
        &[],
        200,
        "provider stored",
        Example::Provider,
    ),
    endpoint(
        "post",
        "/providers/import",
        "Providers",
        "Import provider bundle",
        "Imports provider metadata from a provider bundle.",
        false,
        json_body("Provider bundle.", Example::ProviderBundle),
        &[],
        200,
        "provider bundle imported",
        Example::ProviderBundle,
    ),
];
