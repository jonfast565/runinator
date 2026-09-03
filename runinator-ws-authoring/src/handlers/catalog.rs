use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_models::auth::AuthContext;
use runinator_models::errors::SendableError;
use runinator_models::value::Value;
use runinator_store::roles::DefinitionStore;

use crate::handlers::providers::provider_catalog_item;
use runinator_engine::services::CatalogOperations;
use runinator_ws_core::openapi::docs::{
    CATALOG_FILTERS, EndpointDoc, Example, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_core::{
    ValidatedJson,
    models::{ApiResponse, CatalogQuery},
};
use runinator_ws_middleware::authz::AuthContextExt;

pub async fn get_catalog_items<T: DefinitionStore>(
    Extension(service): Extension<Arc<CatalogOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<CatalogQuery>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::View,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply.into_reply();
    }
    if let Some(uri) = query.uri {
        return match service.fetch(uri.clone()).await {
            Ok(Some(item)) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
            Ok(None) => not_found(format!("Catalog item {uri} not found")),
            Err(err) => api_error(err.to_string()),
        };
    }
    match service.list(query.item_type).await {
        Ok(items) => (StatusCode::OK, Json(ApiResponse::JsonList(items))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn upsert_catalog_item<T: DefinitionStore>(
    Extension(service): Extension<Arc<CatalogOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(item): ValidatedJson<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::CatalogManage,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply.into_reply();
    }
    match service.upsert(item).await {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn seed_builtin_catalog<T: DefinitionStore>(db: &T) -> Result<(), SendableError> {
    for provider in runinator_provider_catalog::metadata() {
        db.upsert_catalog_item(provider_catalog_item(&provider))
            .await?;
    }
    Ok(())
}

/// the `catalog` endpoints.
pub fn routes<T: DefinitionStore>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::get;
    axum::Router::new().route(
        "/catalog/items",
        get(get_catalog_items::<T>)
            .post(upsert_catalog_item::<T>)
            .layer(Extension(pool.clone())),
    )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
        "get",
        "/catalog/items",
        "Catalog",
        "List catalog items",
        "Lists catalog entries such as provider metadata used by authoring clients.",
        false,
        None,
        CATALOG_FILTERS,
        200,
        "catalog items",
        Example::CatalogItem,
    ),
    endpoint!(
        "post",
        "/catalog/items",
        "Catalog",
        "Upsert a catalog item",
        "Creates or replaces a catalog entry.",
        false,
        json_body("Catalog item payload.", Example::CatalogItem),
        &[],
        200,
        "catalog item stored",
        Example::CatalogItem,
    ),
];
