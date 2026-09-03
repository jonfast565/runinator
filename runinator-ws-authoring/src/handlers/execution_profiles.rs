//! Centrally configured, encrypted execution profiles.

use std::sync::Arc;

use axum::{
    Extension, Json,
    body::Body,
    extract::{Path, Query},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use chrono::{Duration, Utc};
use runinator_blob_core::{
    BlobStore, EXECUTION_PROFILE_BUCKET, ListRequest, ObjectKey, PutOptions, blob_uri,
    parse_blob_uri, sha256_hex,
};
use runinator_engine::services::ExecutionProfileOperations;
use runinator_models::{
    auth::{AuthContext, Permission, ResourceType},
    execution_profiles::{
        ExecutionProfile, ExecutionProfileHealth, ExecutionProfilePublishRequest,
        ExecutionProfilePutRequest, ExecutionProfileRevision, ExecutionProfileStatusRequest,
    },
    rbac::{Action, SystemRole},
};
use runinator_secrets::secret_cipher::SecretCipher;
use runinator_store::{
    RuntimeStore,
    roles::{DefinitionStore, ExecutionProfileStore},
};
use runinator_ws_core::{
    ValidatedJson,
    models::ApiResponse,
    openapi::docs::{EndpointDoc, Example, ParamDoc, endpoint, json_body},
    responses::{api_error, bad_request, not_found},
};
use runinator_ws_middleware::authz::{AuthContextExt, AuthorizationStore, AuthzChecker, IntoReply};
use serde::Deserialize;
use tokio::io::AsyncReadExt;
use uuid::Uuid;

const MAX_BUNDLE_BYTES: usize = 10 * 1024 * 1024;

async fn audit<T: AuthorizationStore>(
    db: &T,
    ctx: &AuthContext,
    action: &str,
    id: Uuid,
    detail: String,
) {
    runinator_engine::audit::record_audit(
        db,
        runinator_engine::audit::AuditEntry::new(
            ctx.principal_id,
            ctx.actor_kind(),
            action,
            runinator_engine::audit::AuditOutcome::Success,
            Some("execution_profile"),
            Some(id),
            Some(&detail),
        ),
    )
    .await;
}

#[derive(Debug, Deserialize)]
pub struct ProfileLookup {
    pub name: Option<String>,
    #[serde(default)]
    pub consumer_run_id: Option<Uuid>,
}

async fn run_admitted_profile<T: RuntimeStore>(
    service: &ExecutionProfileOperations<T>,
    run_id: Option<Uuid>,
    profile_id: Uuid,
) -> bool {
    let Some(run_id) = run_id else {
        return false;
    };
    service
        .run_admitted_profile(run_id, profile_id)
        .await
        .unwrap_or(false)
}

fn effective_health(mut profile: ExecutionProfile) -> ExecutionProfile {
    profile.health = if !profile.enabled {
        ExecutionProfileHealth::Disabled
    } else if profile
        .expires_at
        .is_some_and(|expires| expires <= Utc::now())
    {
        ExecutionProfileHealth::Expired
    } else if profile
        .expires_at
        .is_some_and(|expires| expires <= Utc::now() + Duration::minutes(15))
    {
        ExecutionProfileHealth::Expiring
    } else {
        profile.health
    };
    profile
}

pub async fn list<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
) -> (StatusCode, Json<ApiResponse>) {
    if ctx.system_role != Some(SystemRole::Agent)
        && let Err(reply) =
            ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    match service.list(ctx.org_id).await {
        Ok(mut values) => {
            if ctx.system_role != Some(SystemRole::Agent) {
                let visible = match AuthzChecker::new(db.as_ref(), &ctx)
                    .visible_resource_ids(ResourceType::ExecutionProfile)
                    .await
                {
                    Ok(ids) => ids,
                    Err(reply) => return reply.into_reply(),
                };
                if let Some(visible) = visible {
                    values.retain(|value| visible.contains(&value.id));
                }
            }
            (
                StatusCode::OK,
                Json(ApiResponse::ExecutionProfileList(
                    values.into_iter().map(effective_health).collect(),
                )),
            )
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn get_profile<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(query): Query<ProfileLookup>,
) -> (StatusCode, Json<ApiResponse>) {
    let system = matches!(
        ctx.system_role,
        Some(SystemRole::Agent | SystemRole::Worker)
    );
    if !system
        && let Err(reply) =
            ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    if !system
        && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_resource(ResourceType::ExecutionProfile, id, Permission::View)
            .await
    {
        return reply.into_reply();
    }
    if ctx.system_role == Some(SystemRole::Worker)
        && !run_admitted_profile(service.as_ref(), query.consumer_run_id, id).await
    {
        return not_found("execution profile not found");
    }
    match service.fetch(id).await {
        Ok(Some(value)) if value.org_id == ctx.org_id => (
            StatusCode::OK,
            Json(ApiResponse::ExecutionProfile(effective_health(value))),
        ),
        Ok(_) => not_found("execution profile not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn resolve<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Query(query): Query<ProfileLookup>,
) -> (StatusCode, Json<ApiResponse>) {
    let system = matches!(
        ctx.system_role,
        Some(SystemRole::Agent | SystemRole::Worker)
    );
    if !system
        && let Err(reply) =
            ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    let Some(name) = query.name else {
        return bad_request("profile name is required");
    };
    match service.fetch_by_name(ctx.org_id, &name).await {
        Ok(Some(value)) => {
            if ctx.system_role == Some(SystemRole::Worker)
                && !run_admitted_profile(service.as_ref(), query.consumer_run_id, value.id).await
            {
                return not_found("execution profile not found");
            }
            if !system
                && AuthzChecker::new(db.as_ref(), &ctx)
                    .require_resource(ResourceType::ExecutionProfile, value.id, Permission::View)
                    .await
                    .is_err()
            {
                return not_found("execution profile not found");
            }
            (
                StatusCode::OK,
                Json(ApiResponse::ExecutionProfile(effective_health(value))),
            )
        }
        Ok(None) => not_found("execution profile not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn put_profile<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<ExecutionProfilePutRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope()) {
        return reply.into_reply();
    }
    let existing = match service.fetch(id).await {
        Ok(value) => value,
        Err(error) => return api_error(error.to_string()),
    };
    if existing.is_some()
        && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_resource(ResourceType::ExecutionProfile, id, Permission::Edit)
            .await
    {
        return reply.into_reply();
    }
    match service
        .configure(id, ctx.org_id, request, Some(Utc::now()), true)
        .await
    {
        Ok(value) => {
            if existing.is_none()
                && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
                    .grant_resource_owner(ResourceType::ExecutionProfile, id)
                    .await
            {
                return reply.into_reply();
            }
            audit(
                db.as_ref(),
                &ctx,
                "execution_profile.configure",
                id,
                format!(
                    "config_version={} config_digest={}",
                    value.config_version, value.config_digest
                ),
            )
            .await;
            (StatusCode::OK, Json(ApiResponse::ExecutionProfile(value)))
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn publish<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(blobs): Extension<Arc<dyn BlobStore>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    Query(request): Query<ExecutionProfilePublishRequest>,
    body: axum::body::Bytes,
) -> (StatusCode, Json<ApiResponse>) {
    if ctx.system_role != Some(SystemRole::Agent)
        && let Err(reply) =
            ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope())
    {
        return reply.into_reply();
    }
    if body.len() > MAX_BUNDLE_BYTES {
        return bad_request("execution profile bundle exceeds 10 MiB");
    }
    let profile = match service.fetch(id).await {
        Ok(Some(value)) if value.org_id == ctx.org_id && value.enabled => value,
        Ok(_) => return not_found("execution profile not found"),
        Err(error) => return api_error(error.to_string()),
    };
    if ctx.system_role != Some(SystemRole::Agent)
        && let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
            .require_resource(ResourceType::ExecutionProfile, id, Permission::Edit)
            .await
    {
        return reply.into_reply();
    }
    let digest = sha256_hex(&body);
    if !digest.eq_ignore_ascii_case(&request.digest) {
        return bad_request("bundle digest does not match its bytes");
    }
    if profile
        .current_digest
        .as_deref()
        .is_some_and(|value| value.eq_ignore_ascii_case(&digest))
        && let Some(revision) = profile.current_revision
    {
        let _ = service
            .update_health(id, ExecutionProfileHealth::Ready, None)
            .await;
        return match service.fetch_revision(id, revision).await {
            Ok(Some(value)) => (
                StatusCode::OK,
                Json(ApiResponse::ExecutionProfileRevision(value)),
            ),
            Ok(None) => api_error("current execution profile revision is missing"),
            Err(error) => api_error(error.to_string()),
        };
    }
    let revision_number = profile.current_revision.unwrap_or(0) + 1;
    let ciphertext = SecretCipher::from_env().encrypt(&body);
    let key = match ObjectKey::parse(&format!("{id}/{revision_number}.bundle")) {
        Ok(key) => key,
        Err(error) => return bad_request(error.to_string()),
    };
    if let Err(error) = blobs
        .put(
            EXECUTION_PROFILE_BUCKET,
            &key,
            ciphertext,
            PutOptions {
                content_type: Some("application/vnd.runinator.execution-profile+zip".into()),
                ..Default::default()
            },
        )
        .await
    {
        return api_error(error.to_string());
    }
    let revision = ExecutionProfileRevision {
        profile_id: id,
        revision: revision_number,
        digest,
        size_bytes: body.len() as i64,
        publisher_id: ctx.principal_id,
        expires_at: request.expires_at,
        created_at: Utc::now(),
        uri: blob_uri(EXECUTION_PROFILE_BUCKET, &key),
    };
    match service.publish_revision(&revision).await {
        Ok(value) => {
            audit(
                db.as_ref(),
                &ctx,
                "execution_profile.collection_approval_used",
                id,
                format!("config_digest={}", profile.config_digest),
            )
            .await;
            audit(
                db.as_ref(),
                &ctx,
                "execution_profile.publish",
                id,
                format!(
                    "revision={} digest={} size_bytes={}",
                    value.revision, value.digest, value.size_bytes
                ),
            )
            .await;
            (
                StatusCode::CREATED,
                Json(ApiResponse::ExecutionProfileRevision(value)),
            )
        }
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn content<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(blobs): Extension<Arc<dyn BlobStore>>,
    Extension(ctx): Extension<AuthContext>,
    Path((id, revision)): Path<(Uuid, i64)>,
    Query(query): Query<ProfileLookup>,
) -> Response {
    if ctx.system_role != Some(SystemRole::Worker) {
        return (StatusCode::NOT_FOUND, "execution profile bundle not found").into_response();
    }
    if !run_admitted_profile(service.as_ref(), query.consumer_run_id, id).await {
        return (StatusCode::NOT_FOUND, "execution profile bundle not found").into_response();
    }
    let profile = match service.fetch(id).await {
        Ok(Some(value))
            if value.org_id == ctx.org_id
                && value.enabled
                && value.current_revision == Some(revision) =>
        {
            value
        }
        _ => return (StatusCode::NOT_FOUND, "execution profile bundle not found").into_response(),
    };
    if profile
        .expires_at
        .is_some_and(|expiry| expiry <= Utc::now())
    {
        return (StatusCode::GONE, "execution profile has expired").into_response();
    }
    let stored = match service.fetch_revision(id, revision).await {
        Ok(Some(value)) => value,
        _ => return (StatusCode::NOT_FOUND, "execution profile bundle not found").into_response(),
    };
    let Some((bucket, key)) = parse_blob_uri(&stored.uri) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "invalid bundle storage URI",
        )
            .into_response();
    };
    let mut reader = match blobs.open(&bucket, &key, None).await {
        Ok(value) => value.body,
        Err(_) => {
            return (StatusCode::NOT_FOUND, "execution profile bundle not found").into_response();
        }
    };
    let mut ciphertext = Vec::new();
    if reader.read_to_end(&mut ciphertext).await.is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "failed to read bundle").into_response();
    }
    let Some(plaintext) = SecretCipher::from_env().try_decrypt(&ciphertext) else {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "failed to decrypt bundle",
        )
            .into_response();
    };
    if sha256_hex(&plaintext) != stored.digest {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "bundle integrity check failed",
        )
            .into_response();
    }
    audit(
        db.as_ref(),
        &ctx,
        "execution_profile.retrieve",
        id,
        format!("revision={} digest={}", revision, stored.digest),
    )
    .await;
    Response::builder()
        .status(StatusCode::OK)
        .header(
            header::CONTENT_TYPE,
            "application/vnd.runinator.execution-profile+zip",
        )
        .header(header::CONTENT_LENGTH, plaintext.len())
        .body(Body::from(plaintext))
        .unwrap()
}

pub async fn remove<T: AuthorizationStore + DefinitionStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(blobs): Extension<Arc<dyn BlobStore>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope()) {
        return reply.into_reply();
    }
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::ExecutionProfile, id, Permission::Own)
        .await
    {
        return reply.into_reply();
    }
    let profile = match service.fetch(id).await {
        Ok(Some(profile)) if profile.org_id == ctx.org_id => profile,
        Ok(_) => return not_found("execution profile not found"),
        Err(error) => return api_error(error.to_string()),
    };
    let inbound = match service
        .dependent_workflow_paths(id, ctx.org_id, &profile.name)
        .await
    {
        Ok(workflows) => workflows,
        Err(error) => return api_error(error.to_string()),
    };
    if !inbound.is_empty() {
        return bad_request(format!(
            "execution profile {id} is referenced by {}",
            inbound.join(", ")
        ));
    }
    match service.remove(id, ctx.org_id).await {
        Ok(true) => {
            let mut continuation = None;
            loop {
                let page = blobs
                    .list(
                        EXECUTION_PROFILE_BUCKET,
                        &ListRequest {
                            prefix: Some(format!("{id}/")),
                            continuation_token: continuation,
                            ..Default::default()
                        },
                    )
                    .await;
                let Ok(page) = page else { break };
                for object in page.objects {
                    if let Ok(key) = ObjectKey::parse(&object.key) {
                        let _ = blobs.delete(EXECUTION_PROFILE_BUCKET, &key).await;
                    }
                }
                if !page.is_truncated {
                    break;
                }
                continuation = page.next_continuation_token;
            }
            audit(
                db.as_ref(),
                &ctx,
                "execution_profile.revoke",
                id,
                "deleted".into(),
            )
            .await;
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(
                    runinator_models::json!({"success": true}),
                )),
            )
        }
        Ok(false) => not_found("execution profile not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn rotate<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope()) {
        return reply.into_reply();
    }
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::ExecutionProfile, id, Permission::Edit)
        .await
    {
        return reply.into_reply();
    }
    match service.request_refresh(id, ctx.org_id, Utc::now()).await {
        Ok(true) => {
            audit(
                db.as_ref(),
                &ctx,
                "execution_profile.rotate_requested",
                id,
                "desktop refresh requested".into(),
            )
            .await;
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(
                    runinator_models::json!({"success": true}),
                )),
            )
        }
        Ok(false) => not_found("enabled execution profile not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn test_collection<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
) -> (StatusCode, Json<ApiResponse>) {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ctx.selected_scope()) {
        return reply.into_reply();
    }
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(ResourceType::ExecutionProfile, id, Permission::Edit)
        .await
    {
        return reply.into_reply();
    }
    let profile = match service.fetch(id).await {
        Ok(Some(profile)) if profile.org_id == ctx.org_id && profile.enabled => profile,
        Ok(_) => return not_found("enabled execution profile not found"),
        Err(error) => return api_error(error.to_string()),
    };
    match service
        .update_health(profile.id, ExecutionProfileHealth::Unpublished, None)
        .await
    {
        Ok(true) => {
            audit(
                db.as_ref(),
                &ctx,
                "execution_profile.test_requested",
                id,
                "desktop probe requested".into(),
            )
            .await;
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(
                    runinator_models::json!({"success": true}),
                )),
            )
        }
        Ok(false) => not_found("execution profile not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub async fn report_status<T: AuthorizationStore + ExecutionProfileStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(service): Extension<Arc<ExecutionProfileOperations<T>>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<ExecutionProfileStatusRequest>,
) -> (StatusCode, Json<ApiResponse>) {
    if ctx.system_role != Some(SystemRole::Agent) {
        return not_found("execution profile not found");
    }
    let profile = match service.fetch(id).await {
        Ok(Some(profile)) if profile.org_id == ctx.org_id && profile.enabled => profile,
        Ok(_) => return not_found("execution profile not found"),
        Err(error) => return api_error(error.to_string()),
    };
    let error = request.error.map(|value| value.chars().take(512).collect());
    match service
        .update_health(profile.id, request.health, error.clone())
        .await
    {
        Ok(true) => {
            if request.health == ExecutionProfileHealth::Error {
                audit(
                    db.as_ref(),
                    &ctx,
                    "execution_profile.validation_failed",
                    id,
                    error.unwrap_or_else(|| "desktop collection failed".into()),
                )
                .await;
            }
            (
                StatusCode::OK,
                Json(ApiResponse::JsonValue(
                    runinator_models::json!({"success": true}),
                )),
            )
        }
        Ok(false) => not_found("execution profile not found"),
        Err(error) => api_error(error.to_string()),
    }
}

pub fn routes<T: AuthorizationStore + DefinitionStore + ExecutionProfileStore>(
    pool: Arc<T>,
) -> axum::Router {
    use axum::routing::{get, post};
    axum::Router::new()
        .route("/execution_profiles", get(list::<T>))
        .route("/execution_profiles/resolve", get(resolve::<T>))
        .route(
            "/execution_profiles/{id}",
            get(get_profile::<T>)
                .put(put_profile::<T>)
                .delete(remove::<T>),
        )
        .route("/execution_profiles/{id}/publish", post(publish::<T>))
        .route("/execution_profiles/{id}/rotate", post(rotate::<T>))
        .route("/execution_profiles/{id}/test", post(test_collection::<T>))
        .route(
            "/execution_profiles/{id}/status",
            axum::routing::put(report_status::<T>),
        )
        .route(
            "/execution_profiles/{id}/revisions/{revision}/content",
            get(content::<T>),
        )
        .layer(Extension(pool))
}

const PROFILE_NAME_QUERY: &[ParamDoc] = &[ParamDoc {
    name: "name",
    location: "query",
    description: "Execution-profile name to resolve within the active organization.",
    required: true,
    example: "aws-sso",
}];

const PROFILE_PUBLISH_QUERY: &[ParamDoc] = &[
    ParamDoc {
        name: "digest",
        location: "query",
        description: "SHA-256 digest of the plaintext deterministic archive.",
        required: true,
        example: "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
    },
    ParamDoc {
        name: "expires_at",
        location: "query",
        description: "Optional RFC 3339 expiry for the collected login material.",
        required: false,
        example: "2026-09-03T18:00:00Z",
    },
];

/// OpenAPI entries for profile configuration and the private agent/worker data plane.
pub const DOCS: &[EndpointDoc] = &[
    endpoint!(
        "get",
        "/execution_profiles",
        "Execution profiles",
        "List execution profiles",
        "Lists profile definitions and publication status without exposing collected contents.",
        false,
        None,
        &[],
        200,
        "execution-profile list",
        Example::None,
    ),
    endpoint!(
        "get",
        "/execution_profiles/resolve",
        "Execution profiles",
        "Resolve an execution profile",
        "Resolves an authored profile name within the active organization.",
        false,
        None,
        PROFILE_NAME_QUERY,
        200,
        "resolved execution profile",
        Example::None,
    ),
    endpoint!(
        "get",
        "/execution_profiles/{id}",
        "Execution profiles",
        "Get an execution profile",
        "Returns one profile definition and its safe publication metadata.",
        false,
        None,
        &[],
        200,
        "execution profile",
        Example::None,
    ),
    endpoint!(
        "put",
        "/execution_profiles/{id}",
        "Execution profiles",
        "Configure an execution profile",
        "Creates or updates provider-agnostic collection and worker-exposure rules.",
        false,
        json_body("Execution-profile configuration.", Example::None),
        &[],
        200,
        "configured execution profile",
        Example::None,
    ),
    endpoint!(
        "delete",
        "/execution_profiles/{id}",
        "Execution profiles",
        "Revoke an execution profile",
        "Revokes the profile and removes its encrypted archive revisions.",
        false,
        None,
        &[],
        200,
        "profile revoked",
        Example::TaskResponse,
    ),
    endpoint!(
        "post",
        "/execution_profiles/{id}/publish",
        "Execution profiles",
        "Publish collected profile files",
        "Accepts a deterministic archive from an authorized desktop agent and stores it encrypted.",
        false,
        None,
        PROFILE_PUBLISH_QUERY,
        201,
        "published revision metadata",
        Example::None,
    ),
    endpoint!(
        "post",
        "/execution_profiles/{id}/rotate",
        "Execution profiles",
        "Request profile rotation",
        "Requests an interactive desktop refresh and a new atomic publication.",
        false,
        None,
        &[],
        200,
        "rotation requested",
        Example::TaskResponse,
    ),
    endpoint!(
        "post",
        "/execution_profiles/{id}/test",
        "Execution profiles",
        "Test profile collection",
        "Requests that an approved desktop agent probe and recollect the profile.",
        false,
        None,
        &[],
        200,
        "test requested",
        Example::TaskResponse,
    ),
    endpoint!(
        "put",
        "/execution_profiles/{id}/status",
        "Execution profiles",
        "Report collection status",
        "Allows an authorized desktop agent to report sanitized collection health.",
        false,
        json_body("Sanitized profile health report.", Example::None),
        &[],
        200,
        "status recorded",
        Example::TaskResponse,
    ),
    endpoint!(
        "get",
        "/execution_profiles/{id}/revisions/{revision}/content",
        "Execution profiles",
        "Retrieve a profile revision",
        "Streams authenticated plaintext archive bytes only to an authorized worker immediately before execution.",
        false,
        None,
        &[],
        200,
        "profile archive",
        Example::None,
    ),
];
