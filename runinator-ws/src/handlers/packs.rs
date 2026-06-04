use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Bytes,
    http::{HeaderMap, StatusCode, header},
};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::bundles::{PackImportResult, SecretBundle};
use runinator_models::workflows::WorkflowBundle;

use crate::events::{AppEvent, EventSender, emit};
use crate::handlers::credentials::import_secret_entries;
use crate::handlers::workflows::{
    json_workflow_import_risk_acknowledged, json_workflow_import_risk_required,
};
use crate::models::ApiResponse;
use crate::repository;
use crate::responses::{api_error, bad_request};

// import a compiled pack zip, or a raw workflow bundle json when risk is acknowledged.
pub(crate) async fn import_pack<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(events): Extension<EventSender>,
    headers: HeaderMap,
    body: Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if is_json_content_type(&headers) {
        if !json_workflow_import_risk_acknowledged(&headers) {
            return json_workflow_import_risk_required();
        }
        let bundle: WorkflowBundle = match serde_json::from_slice(&body) {
            Ok(bundle) => bundle,
            Err(err) => return bad_request(format!("invalid workflow bundle json: {err}")),
        };
        log::info!(
            "Importing json workflow bundle through pack endpoint: {} workflows, {} triggers",
            bundle.workflows.len(),
            bundle.triggers.len()
        );
        let workflows = match repository::import_workflow_bundle(db.as_ref(), bundle).await {
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
        "Importing pack: {} workflows, {} triggers, {} secrets",
        workflow_bundle.workflows.len(),
        workflow_bundle.triggers.len(),
        secret_bundle
            .as_ref()
            .map(|bundle| bundle.secrets.len())
            .unwrap_or(0),
    );
    let workflows = match repository::import_workflow_bundle(db.as_ref(), workflow_bundle).await {
        Ok(bundle) => bundle,
        Err(err) => return api_error(err.to_string()),
    };
    let secrets = match &secret_bundle {
        Some(bundle) => match import_secret_entries(bundle) {
            Ok(imported) => SecretBundle { secrets: imported },
            Err(error) => return error.into_response(),
        },
        None => SecretBundle::default(),
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
