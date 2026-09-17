use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Bytes,
    extract::{Path, Query},
    http::StatusCode,
};
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

use runinator_engine::services::{PackImportRequest, PackOperations, PackReadinessRequest};
use runinator_ws_core::models::ApiResponse;
use runinator_ws_core::openapi::docs::{
    EndpointDoc, Example, PACK_IMPORT_PARAMS, RequestDoc, endpoint,
};
use runinator_ws_core::responses::{api_error, bad_request};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore};

const AI_MISSIONS_STARTER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/ai-missions.zip"));
const AI_MISSIONS_KEY: &str = "ai-missions";
const AI_MISSIONS_VERSION: u32 = 1;

pub async fn list_starter_packs<T: DefinitionStore + ExecutionProfileStore + AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(packs): Extension<Arc<PackOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    let visible = match runinator_ws_middleware::authz::AuthzChecker::new(db.as_ref(), &ctx)
        .visible_pipeline_ids()
        .await
    {
        Ok(value) => value,
        Err(reply) => return reply,
    };
    let installed = match packs
        .is_ready(PackReadinessRequest {
            org_id: ctx.org_id,
            pipeline_namespace: "runinator.missions",
            pipeline_keys: &["coding_mission", "research_report_mission"],
            execution_profile: "claude",
            visible_pipeline_ids: visible.as_ref(),
        })
        .await
    {
        Ok(installed) => installed,
        Err(error) => return api_error(error.to_string()),
    };
    (
        StatusCode::OK,
        Json(ApiResponse::JsonValue(
            serde_json::json!([{
                "key": AI_MISSIONS_KEY,
                "name": "AI missions",
                "version": AI_MISSIONS_VERSION,
                "state": if installed { "installed" } else { "missing" },
                "required_profile": "claude"
            }])
            .into(),
        )),
    )
}

pub async fn install_starter_pack<
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
    Path(key): Path<String>,
) -> (StatusCode, Json<ApiResponse>) {
    if key != AI_MISSIONS_KEY {
        return bad_request(format!("unknown starter pack '{key}'"));
    }
    import_pack::<T>(
        Extension(packs),
        Extension(ctx),
        Query(PackImportParams {
            contract_override_reason: None,
            overwrite: false,
        }),
        Bytes::from_static(AI_MISSIONS_STARTER),
    )
    .await
}

// query parameters for the pack import endpoint.
#[derive(Debug, Default, Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PackImportParams {
    pub contract_override_reason: Option<String>,
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
        return reply.into_reply();
    }
    if params.contract_override_reason.is_some()
        && let Err(reply) = ctx.require_scope_action(Action::Own, import_scope)
    {
        return reply.into_reply();
    }
    let overwrite = params.overwrite;
    let contents = match runinator_pack_wire::pack::read_pack_zip(&body) {
        Ok(parsed) => parsed,
        Err(err) => return bad_request(format!("invalid pack zip: {err}")),
    };
    let settings_section = contents.settings.as_ref();
    // settings carry a nullable `org_id` all the way to the store, so a platform-scoped import is
    // representable. authorization is already the gate: `import_scope` is `ScopeRef::PLATFORM`
    // without an organization, and the SecretsWrite and CredentialsManage checks below run against
    // it, so only a platform admin can land platform settings.
    if settings_section.is_some_and(|bundle| {
        bundle
            .settings
            .iter()
            .any(|entry| entry.kind == runinator_models::settings::SettingKind::Secret)
    }) && let Err(reply) = ctx.require_scope_action(Action::SecretsWrite, import_scope)
    {
        return reply.into_reply();
    }
    if settings_section.is_some_and(|bundle| !bundle.execution_profiles.is_empty())
        && let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, import_scope)
    {
        return reply.into_reply();
    }
    if (!contents.functions.is_empty() || !contents.function_artifacts.is_empty())
        && let Err(reply) = ctx.require_scope_action(Action::FunctionsManage, import_scope)
    {
        return reply.into_reply();
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
        .import_compiled_pack(PackImportRequest {
            contract_override_reason: params.contract_override_reason,
            workflows: workflow_bundle,
            settings: secret_bundle.as_ref(),
            pipelines: pipeline_bundle.as_ref(),
            functions: &contents.functions,
            artifacts: &artifacts,
            import_org,
            owner,
            created_by: ctx.principal_id,
            overwrite,
        })
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
    pool: std::sync::Arc<T>,
) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route(
            runinator_models::api_routes::API_PACKS_IMPORT,
            post(import_pack::<T>),
        )
        .route(
            runinator_models::api_routes::API_STARTER_PACKS,
            get(list_starter_packs::<T>),
        )
        .route(
            runinator_models::api_routes::API_STARTER_PACK_INSTALL,
            post(install_starter_pack::<T>),
        )
        .layer(Extension(pool))
}

/// the openapi entries for the routes above.
pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
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
    ),
    endpoint!(
        "get",
        "/starter-packs",
        "Packs",
        "List starter packs",
        "Lists product-owned packs that can be installed explicitly.",
        false,
        None,
        &[],
        200,
        "starter pack catalog",
        Example::StarterPackList,
    ),
    endpoint!(
        "post",
        "/starter-packs/{key}/install",
        "Packs",
        "Install a starter pack",
        "Explicitly imports one embedded compiled starter pack.",
        false,
        None,
        &[],
        200,
        "pack import result",
        Example::WorkflowBundle,
    ),
];
