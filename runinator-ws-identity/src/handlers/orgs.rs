use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_models::auth::AuthContext;
use runinator_models::orgs::{
    AddOrgMemberRequest, CreateOrgRequest, OrgContextResponse, OrgMembershipView, OrgRole,
    SwitchOrgRequest, UpdateOrgMemberRequest, UpdateOrgRequest, slugify,
};
use runinator_models::value::Value;
use runinator_store::{RuntimeStore, roles::OrgStore};
use serde::Serialize;
use uuid::Uuid;

fn org_scope(org_id: Uuid) -> runinator_models::rbac::ScopeRef {
    runinator_models::rbac::ScopeRef::new(
        runinator_models::rbac::ScopeKind::Organization,
        Some(org_id),
    )
    .unwrap()
}

use runinator_ws_core::ValidatedJson;
use runinator_ws_core::models::{ApiError, ApiResponse};
use runinator_ws_core::responses::{api_error, bad_request, not_found};
use runinator_ws_middleware::auth::{AuthConfig, issue_access_token};
use runinator_ws_middleware::authz::{AuthContextExt, GuardError, IntoReply};

type Reply = (StatusCode, Json<ApiResponse>);

fn forbidden() -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(ApiError::new(
            "insufficient permission for this organization",
        ))),
    )
}

fn ok_value<T: Serialize>(value: &T) -> Reply {
    match serde_json::to_value(value) {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(Value::from(value))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}
fn require_principal(ctx: &AuthContext) -> Result<Uuid, GuardError> {
    ctx.principal_id.ok_or_else(|| {
        {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiResponse::ApiError(ApiError::new(
                    "this action requires a user principal",
                ))),
            )
        }
        .into()
    })
}

/// create an organization. the creating user becomes its owner (self-serve signup).
pub async fn create_org<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreateOrgRequest>,
) -> Reply {
    let user_id = match require_principal(&ctx) {
        Ok(id) => id,
        Err(reply) => return reply.into_reply(),
    };
    let name = request.name.trim().to_string();
    if name.is_empty() {
        return bad_request("organization name must not be empty");
    }
    let slug = request
        .slug
        .map(|raw| slugify(&raw))
        .unwrap_or_else(|| slugify(&name));
    if slug.is_empty() {
        return bad_request("organization slug resolves to empty; provide an explicit slug");
    }
    match db.fetch_org_by_slug(slug.clone()).await {
        Ok(Some(_)) => return bad_request(format!("slug '{slug}' is already taken")),
        Ok(None) => {}
        Err(err) => return api_error(err.to_string()),
    }
    let org = match db.create_org(name, slug).await {
        Ok(org) => org,
        Err(err) => return api_error(err.to_string()),
    };
    let Some(org_id) = org.id else {
        return api_error("created org is missing an id");
    };
    if let Err(err) = db.add_org_member(org_id, user_id, OrgRole::Owner).await {
        return api_error(err.to_string());
    }
    ok_value(&org)
}

/// list every org (platform-admin view).
pub async fn list_orgs<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::Own,
        runinator_models::rbac::ScopeRef::PLATFORM,
    ) {
        return reply.into_reply();
    }
    match db.list_orgs().await {
        Ok(orgs) => ok_value(&orgs),
        Err(err) => api_error(err.to_string()),
    }
}

/// the caller's org memberships, each with their role.
pub async fn list_my_orgs<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let user_id = match require_principal(&ctx) {
        Ok(id) => id,
        Err(reply) => return reply.into_reply(),
    };
    match db.list_user_orgs(user_id).await {
        Ok(orgs) => {
            let views: Vec<OrgMembershipView> = orgs
                .into_iter()
                .map(|(org, role)| OrgMembershipView { org, role })
                .collect();
            ok_value(&views)
        }
        Err(err) => api_error(err.to_string()),
    }
}

/// fetch one org (any member, or platform admin).
pub async fn get_org<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, org_scope(org_id))
    {
        return reply.into_reply();
    }
    match db.fetch_org(org_id).await {
        Ok(Some(org)) => ok_value(&org),
        Ok(None) => not_found("organization not found"),
        Err(err) => api_error(err.to_string()),
    }
}

/// rename or (dis|en)able an org (org admin, or platform admin).
pub async fn update_org<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<UpdateOrgRequest>,
) -> Reply {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::Own, org_scope(org_id))
    {
        return reply.into_reply();
    }
    let name = request.name.map(|n| n.trim().to_string());
    if matches!(name.as_deref(), Some("")) {
        return bad_request("organization name must not be empty");
    }
    match db.update_org(org_id, name, request.disabled).await {
        Ok(org) => ok_value(&org),
        Err(err) => api_error(err.to_string()),
    }
}

/// delete an org and its memberships (org owner, or platform admin).
pub async fn delete_org<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::Own, org_scope(org_id))
    {
        return reply.into_reply();
    }
    match db.delete_org(org_id).await {
        Ok(()) => ok_value(&serde_json::json!({ "deleted": org_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// list an org's members (any member, or platform admin).
pub async fn list_org_members<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) =
        ctx.require_scope_action(runinator_models::rbac::Action::View, org_scope(org_id))
    {
        return reply.into_reply();
    }
    match db.list_org_members(org_id).await {
        Ok(members) => ok_value(&members),
        Err(err) => api_error(err.to_string()),
    }
}

/// add or re-role a member (org admin, or platform admin).
pub async fn add_org_member<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<AddOrgMemberRequest>,
) -> Reply {
    let action = if request.role == OrgRole::Owner {
        runinator_models::rbac::Action::Own
    } else {
        runinator_models::rbac::Action::MembersManage
    };
    if let Err(reply) = ctx.require_scope_action(action, org_scope(org_id)) {
        return reply.into_reply();
    }
    if db
        .fetch_org(org_id)
        .await
        .map(|o| o.is_none())
        .unwrap_or(false)
    {
        return not_found("organization not found");
    }
    match db
        .add_org_member(org_id, request.user_id, request.role)
        .await
    {
        Ok(()) => ok_value(&serde_json::json!({ "org_id": org_id, "user_id": request.user_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// change a member's role (org admin, or platform admin).
pub async fn update_org_member<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    ValidatedJson(request): ValidatedJson<UpdateOrgMemberRequest>,
) -> Reply {
    let action = if request.role == OrgRole::Owner {
        runinator_models::rbac::Action::Own
    } else {
        runinator_models::rbac::Action::MembersManage
    };
    if let Err(reply) = ctx.require_scope_action(action, org_scope(org_id)) {
        return reply.into_reply();
    }
    // guard the last owner: an org must always retain at least one owner.
    if let Err(reply) = guard_last_owner(db.as_ref(), org_id, user_id, request.role).await {
        return reply.into_reply();
    }
    match db.add_org_member(org_id, user_id, request.role).await {
        Ok(()) => ok_value(&serde_json::json!({ "org_id": org_id, "user_id": user_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// remove a member (org admin, or platform admin).
pub async fn remove_org_member<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(
        runinator_models::rbac::Action::MembersManage,
        org_scope(org_id),
    ) {
        return reply.into_reply();
    }
    // removing an owner demotes them out of the org; block if they are the last one.
    if let Err(reply) = guard_last_owner(db.as_ref(), org_id, user_id, OrgRole::Member).await {
        return reply.into_reply();
    }
    match db.remove_org_member(org_id, user_id).await {
        Ok(()) => ok_value(&serde_json::json!({ "removed": user_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// switch the active org: re-issue an access token bound to `org_id` and the caller's role there.
pub async fn switch_org<T: OrgStore + RuntimeStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(config): Extension<Arc<AuthConfig>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<SwitchOrgRequest>,
) -> Reply {
    let user_id = match require_principal(&ctx) {
        Ok(id) => id,
        Err(reply) => return reply.into_reply(),
    };
    let membership = match db.fetch_org_membership(request.org_id, user_id).await {
        Ok(Some(membership)) => membership,
        Ok(None) => return forbidden(),
        Err(err) => return api_error(err.to_string()),
    };
    let org = match db.fetch_org(request.org_id).await {
        Ok(Some(org)) => org,
        Ok(None) => return not_found("organization not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if org.disabled {
        return forbidden();
    }
    let (access_token, _exp) = match issue_access_token(
        &config,
        user_id,
        match ctx.session_id {
            Some(id) => id,
            None => return forbidden(),
        },
        Some(request.org_id),
    ) {
        Ok(pair) => pair,
        Err(err) => return api_error(err),
    };
    ok_value(&OrgContextResponse {
        access_token,
        expires_in: config.access_ttl_secs,
        org,
        role: membership.role,
    })
}

/// reject a role change/removal that would leave `org_id` with no owner.
async fn guard_last_owner<T: OrgStore + RuntimeStore>(
    db: &T,
    org_id: Uuid,
    user_id: Uuid,
    new_role: OrgRole,
) -> Result<(), Reply> {
    if new_role == OrgRole::Owner {
        return Ok(());
    }
    let members = db
        .list_org_members(org_id)
        .await
        .map_err(|err| api_error(err.to_string()))?;
    let is_target_owner = members
        .iter()
        .any(|m| m.user_id == user_id && m.role == OrgRole::Owner);
    if !is_target_owner {
        return Ok(());
    }
    let owner_count = members.iter().filter(|m| m.role == OrgRole::Owner).count();
    if owner_count <= 1 {
        return Err(bad_request(
            "an organization must retain at least one owner",
        ));
    }
    Ok(())
}

/// the `orgs` endpoints.
pub fn routes<T: OrgStore + RuntimeStore>(pool: std::sync::Arc<T>) -> axum::Router {
    use axum::Extension;
    use axum::routing::{get, patch, post};
    axum::Router::new()
        .route(
            "/auth/switch-org",
            post(switch_org::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/orgs",
            get(list_orgs::<T>)
                .post(create_org::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/me",
            get(list_my_orgs::<T>).layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}",
            get(get_org::<T>)
                .patch(update_org::<T>)
                .delete(delete_org::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/members",
            get(list_org_members::<T>)
                .post(add_org_member::<T>)
                .layer(Extension(pool.clone())),
        )
        .route(
            "/orgs/{id}/members/{user_id}",
            patch(update_org_member::<T>)
                .delete(remove_org_member::<T>)
                .layer(Extension(pool.clone())),
        )
}
