use std::sync::Arc;

use axum::{Extension, Json, extract::Query, http::StatusCode};
use runinator_models::auth::AuthContext;
use runinator_models::errors::SendableError;
use runinator_models::json;
use runinator_models::value::Value;
use runinator_store::roles::DefinitionStore;

use crate::handlers::providers::provider_catalog_item;
use runinator_engine::services::CatalogOperations;
use runinator_ws_core::models::{ApiResponse, CatalogQuery};
use runinator_ws_core::openapi::docs::{
    CATALOG_FILTERS, EndpointDoc, Example, endpoint, json_body,
};
use runinator_ws_core::responses::{api_error, not_found};
use runinator_ws_middleware::authz::AuthContextExt;

pub async fn get_catalog_items<T: DefinitionStore>(
    Extension(service): Extension<Arc<CatalogOperations<T>>>,
    Extension(_ctx): Extension<AuthContext>,
    Query(query): Query<CatalogQuery>,
) -> (StatusCode, Json<ApiResponse>) {
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
    Json(item): Json<Value>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::CatalogManage,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply;
    }
    match service.upsert(item).await {
        Ok(item) => (StatusCode::OK, Json(ApiResponse::JsonValue(item))),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn seed_builtin_catalog<T: DefinitionStore>(db: &T) -> Result<(), SendableError> {
    for raw in [include_str!("../../../packs/sdlc/sdlc.rrx")] {
        let item = rexrap_pack_catalog_item(raw)?;
        db.upsert_catalog_item(item).await?;
    }
    for provider in runinator_provider_catalog::metadata() {
        db.upsert_catalog_item(provider_catalog_item(&provider))
            .await?;
    }
    Ok(())
}

fn rexrap_pack_catalog_item(raw: &str) -> Result<Value, SendableError> {
    let blocks = runinator_rexrap::parse_rrx_blocks(raw)?;
    let package = blocks
        .packages
        .first()
        .ok_or_else(|| "builtin pack is missing a package block".to_string())?;
    let manifest: Value = serde_json::from_str(package)?;
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
        "item_type": "rexrap_pack",
        "name": manifest.get("name").and_then(Value::as_str).unwrap_or("SDLC Automation Pack"),
        "version": version,
        "document": {
            "workflows": manifest.get("workflows").cloned().unwrap_or_else(|| json!([])),
            "triggers": manifest.get("triggers").cloned().unwrap_or_else(|| json!([]))
        }
    }))
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
    endpoint(
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
    endpoint(
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
