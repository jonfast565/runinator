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
    // a platform admin imports globally; an org admin imports into their active org. imported
    // workflows are stamped with `import_org` so the pack lands in the right tenant.
    let import_org = ctx.org_id;
    if let Some(org_id) = import_org {
        if let Err(reply) = crate::authz::require_org_admin(&ctx, org_id) {
            return reply;
        }
    } else if let Err(reply) = crate::authz::require_admin(&ctx) {
        return reply;
    }
    let overwrite = params.overwrite;
    if is_json_content_type(&headers) {
        if !json_workflow_import_risk_acknowledged(&headers) {
            return json_workflow_import_risk_required();
        }
        let mut bundle: WorkflowBundle = match serde_json::from_slice(&body) {
            Ok(bundle) => bundle,
            Err(err) => return bad_request(format!("invalid workflow bundle json: {err}")),
        };
        stamp_bundle_org(&mut bundle, import_org);
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
                pipelines: Vec::new(),
            })),
        );
    }

    let contents = match runinator_utilities::pack::read_pack_zip(&body) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(format!("invalid pack zip: {err}")),
    };
    let mut workflow_bundle = contents.workflows;
    let secret_bundle = contents.secrets;
    let pipeline_bundle = contents.pipelines;
    stamp_bundle_org(&mut workflow_bundle, import_org);
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
    // import pipelines after workflows so member names resolve to freshly-imported ids, and their
    // links materialize as managed chained triggers stamped with the pipeline id.
    let pipelines = match &pipeline_bundle {
        Some(bundle) => {
            match repository::import_pipeline_bundle_with(db.as_ref(), bundle, import_org).await {
                Ok(imported) => imported,
                Err(err) => return api_error(err.to_string()),
            }
        }
        None => Vec::new(),
    };
    if let Some(bundle) = &pipeline_bundle {
        log::info!("Imported {} pipelines from pack", bundle.pipelines.len());
    }
    emit(&events, AppEvent::WorkflowsChanged);
    (
        StatusCode::OK,
        Json(ApiResponse::PackImport(PackImportResult {
            workflows,
            secrets,
            pipelines,
        })),
    )
}

// stamp every workflow in an imported bundle with the target org so it lands in the caller's tenant.
fn stamp_bundle_org(bundle: &mut WorkflowBundle, org_id: Option<uuid::Uuid>) {
    for workflow in &mut bundle.workflows {
        workflow.org_id = org_id;
    }
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
