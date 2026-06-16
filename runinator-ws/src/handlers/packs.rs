use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Bytes,
    extract::Query,
    http::{HeaderMap, StatusCode, header},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::workflows::WorkflowBundle;
use runinator_models::{
    auth::AuthContext,
    bundles::{PackImportResult, SecretBundle},
};
use serde::Deserialize;
use utoipa::IntoParams;

use crate::events::{AppEvent, EventSender, emit};
use crate::handlers::credentials::import_secret_entries_with;
use crate::handlers::workflows::{
    json_workflow_import_risk_acknowledged, json_workflow_import_risk_required,
};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request};

// query parameters for the pack import endpoint.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub(crate) struct PackImportParams {
    // when true, an explicit re-apply updates existing items in place instead of skipping ones
    // that are not strictly newer than the stored copy.
    #[serde(default)]
    overwrite: bool,
}

// import a compiled pack zip, or a raw workflow bundle json when risk is acknowledged.
#[utoipa::path(
    post,
    path = "/packs/import",
    tag = "Packs",
    params(
        PackImportParams,
        (
            "x-runinator-json-workflow-risk",
            Header,
            description = "Required only for raw JSON workflow bundle imports posted as application/json.",
            example = "system-breakage-possible"
        )
    ),
    request_body(
        description = "A compiled pack zip produced by `runinatorctl workflows apply`, or a raw JSON workflow bundle when the risk-acknowledgment header is present.",
        content(
            ("application/zip"),
            ("application/json")
        )
    ),
    responses(
        (status = 200, description = "pack or workflow bundle imported", body = serde_json::Value),
        (status = 400, description = "invalid zip, invalid json, or missing risk acknowledgment", body = crate::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = crate::models::ApiError),
    ),
)]
pub(crate) async fn import_pack<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    Extension(ctx): Extension<AuthContext>,
    Query(params): Query<PackImportParams>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = crate::authz::require_admin(&ctx) {
        return reply;
    }
    let overwrite = params.overwrite;
    if is_json_content_type(&headers) {
        if !json_workflow_import_risk_acknowledged(&headers) {
            return json_workflow_import_risk_required();
        }
        let bundle: WorkflowBundle = match serde_json::from_slice(&body) {
            Ok(bundle) => bundle,
            Err(err) => return bad_request(format!("invalid workflow bundle json: {err}")),
        };
        log::info!(
            "Importing json workflow bundle through pack endpoint: {} workflows, {} triggers (overwrite={overwrite})",
            bundle.workflows.len(),
            bundle.triggers.len()
        );
        let workflows =
            match repository::import_workflow_bundle_with(db.as_ref(), bundle, overwrite).await {
                Ok(bundle) => bundle,
                Err(err) => return api_error(err.to_string()),
            };
        emit(&events, AppEvent::WorkflowsChanged);
        return (
            StatusCode::OK,
            Json(ApiResponse::PackImport(PackImportResult {
                workflows,
                secrets: SecretBundle::default(),
            })),
        );
    }

    let (workflow_bundle, secret_bundle) = match runinator_utilities::pack::read_pack_zip(&body) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(format!("invalid pack zip: {err}")),
    };
    log::info!(
        "Importing pack: {} workflows, {} triggers, {} secrets (overwrite={overwrite})",
        workflow_bundle.workflows.len(),
        workflow_bundle.triggers.len(),
        secret_bundle
            .as_ref()
            .map(|bundle| bundle.secrets.len())
            .unwrap_or(0),
    );
    // apply config/secrets before workflows so a pack's own `config.*` values are present in the
    // store when its workflows are type-checked on import.
    let secrets = match &secret_bundle {
        Some(bundle) => match import_secret_entries_with(db.as_ref(), bundle, overwrite).await {
            Ok(imported) => SecretBundle { secrets: imported },
            Err(error) => return error.into_response(),
        },
        None => SecretBundle::default(),
    };
    let workflows = match repository::import_workflow_bundle_with(
        db.as_ref(),
        workflow_bundle,
        overwrite,
    )
    .await
    {
        Ok(bundle) => bundle,
        Err(err) => return api_error(err.to_string()),
    };
    emit(&events, AppEvent::WorkflowsChanged);
    (
        StatusCode::OK,
        Json(ApiResponse::PackImport(PackImportResult {
            workflows,
            secrets,
        })),
    )
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|kind| kind.trim() == "application/json")
        })
}
