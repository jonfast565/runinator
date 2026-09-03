use std::sync::Arc;

use axum::{Extension, Json, body::Bytes, extract::Query, http::StatusCode};
use runinator_models::{
    auth::{AuthContext, PrincipalKind},
    rbac::{Action, ScopeKind, ScopeRef},
    workflows::WorkflowBundle,
};
use runinator_store::{
    PackTransactionStore, RuntimeStore,
    roles::{
        DefinitionStore, ExecutionProfileStore, FunctionStore, NotificationStore, ScheduleStore,
        SettingStore,
    },
};
use serde::Deserialize;
use utoipa::IntoParams;

use runinator_engine::services::PackOperations;
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, PACK_IMPORT_PARAMS, RequestDoc, endpoint,
};
use runinator_ws_core::responses::{api_error, bad_request};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore};

// query parameters for the pack import endpoint.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PackImportParams {
    // when true, an explicit re-apply updates existing items in place instead of skipping ones
    // that are not strictly newer than the stored copy.
    #[serde(default)]
    overwrite: bool,
}

// import a compiled pack zip.
#[utoipa::path(
    post,
    path = "/packs/import",
    tag = "Packs",
    params(PackImportParams),
    request_body(
        description = "A compiled pack zip produced by `runinatorctl workflows apply`.",
        content(("application/zip"))
    ),
    responses(
        (status = 200, description = "pack imported", body = serde_json::Value),
        (status = 400, description = "invalid pack zip", body = runinator_ws_core::models::ApiError),
        (status = 401, description = "request is missing or has an invalid credential", body = runinator_ws_core::models::ApiError),
    ),
)]
pub async fn import_pack<
    T: DefinitionStore
        + AuthorizationStore
        + RuntimeStore
        + PackTransactionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + SettingStore
        + ExecutionProfileStore,
>(
    Extension(packs): Extension<Arc<PackOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(params): Query<PackImportParams>,
    body: Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    // a platform admin imports globally; an org admin imports into their active org. imported
    // workflows are stamped with `import_org` so the pack lands in the right tenant.
    let import_org = ctx.org_id;
    let import_scope = import_org
        .and_then(|id| ScopeRef::new(ScopeKind::Organization, Some(id)))
        .unwrap_or(ScopeRef::PLATFORM);
    let owner = if import_scope.kind == ScopeKind::Platform {
        ScopeRef::PLATFORM
    } else {
        match (ctx.kind, ctx.principal_id) {
            (PrincipalKind::User, Some(id)) => ScopeRef::new(ScopeKind::User, Some(id)).unwrap(),
            _ => import_scope,
        }
    };
    if let Err(reply) = ctx.require_scope_action(Action::Edit, import_scope) {
        return reply;
    }
    let overwrite = params.overwrite;
    let contents = match runinator_pack_wire::pack::read_pack_zip(&body) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(format!("invalid pack zip: {err}")),
    };
    let settings_section = contents.settings.as_ref();
    if import_org.is_none() && settings_section.is_some_and(|bundle| !bundle.settings.is_empty()) {
        return bad_request("pack settings must be imported into an organization");
    }
    if settings_section.is_some_and(|bundle| {
        bundle
            .settings
            .iter()
            .any(|entry| entry.kind == runinator_models::settings::SettingKind::Secret)
    }) && let Err(reply) = ctx.require_scope_action(Action::SecretsWrite, import_scope)
    {
        return reply;
    }
    if settings_section.is_some_and(|bundle| !bundle.execution_profiles.is_empty())
        && let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, import_scope)
    {
        return reply;
    }
    if (!contents.functions.is_empty() || !contents.function_artifacts.is_empty())
        && let Err(reply) = ctx.require_scope_action(Action::FunctionsManage, import_scope)
    {
        return reply;
    }
    let mut workflow_bundle = contents.workflows;
    let secret_bundle = contents.settings;
    let pipeline_bundle = contents.pipelines;
    stamp_bundle_org(&mut workflow_bundle, import_org);
    log::info!(
        "Importing pack: {} workflows, {} triggers, {} secrets (overwrite={overwrite})",
        workflow_bundle.workflows.len(),
        workflow_bundle.triggers.len(),
        secret_bundle
            .as_ref()
            .map(|bundle| bundle.settings.len())
            .unwrap_or(0),
    );
    // packaged functions land before workflows, for the same reason secrets do: a workflow in this
    // pack may bind to one, and import-time binding validation would reject it against a catalog
    // that did not yet know the package.
    //
    // artifacts first, then the publishes that reference them — a publish naming bytes the server
    // does not hold is refused, which is what keeps a half-imported pack from leaving versions
    // nothing can run.
    let mut artifacts = Vec::with_capacity(contents.function_artifacts.len());
    for (digest, bytes) in &contents.function_artifacts {
        match packs.stage_function_artifact(digest, bytes.clone()).await {
            Ok(artifact) => artifacts.push(artifact),
            Err(error) => {
                return api_error(format!(
                    "pack artifact {digest} could not be stored: {error}"
                ));
            }
        }
    }
    let result = match packs
        .import_compiled_pack(
            workflow_bundle,
            secret_bundle.as_ref(),
            pipeline_bundle.as_ref(),
            &contents.functions,
            &artifacts,
            import_org,
            owner,
            ctx.principal_id,
            overwrite,
        )
        .await
    {
        Ok(result) => result,
        Err(error) if error.bad_request => return bad_request(error.message),
        Err(error) => return api_error(error.message),
    };
    packs.workflows_changed(import_org);
    (StatusCode::OK, Json(ApiResponse::PackImport(result)))
}

// stamp every workflow in an imported bundle with the target org so it lands in the caller's tenant.
fn stamp_bundle_org(bundle: &mut WorkflowBundle, org_id: Option<uuid::Uuid>) {
    for workflow in &mut bundle.workflows {
        workflow.org_id = org_id;
    }
}

/// the `packs` endpoints.
pub fn routes<
    T: DefinitionStore
        + AuthorizationStore
        + RuntimeStore
        + PackTransactionStore
        + FunctionStore
        + NotificationStore
        + ScheduleStore
        + SettingStore
        + ExecutionProfileStore,
>(
    _pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::routing::post;
    axum::Router::new().route(
        runinator_models::api_routes::API_PACKS_IMPORT,
        post(import_pack::<T>),
    )
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[endpoint(
    "post",
    "/packs/import",
    "Packs",
    "Import a compiled pack zip",
    "Imports a compiled pack zip containing `workflows.json` and optional versioned `settings.json`. Legacy `secrets.json` remains readable for one compatibility release. The backend reads compiled JSON only; it does not compile REXRAP.",
    false,
    Some(RequestDoc {
        description: "Compiled pack zip.",
        example: Example::WorkflowBundle,
        content_type: "application/zip",
    }),
    PACK_IMPORT_PARAMS,
    200,
    "pack import result",
    Example::WorkflowBundle,
)];
