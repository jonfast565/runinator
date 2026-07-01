use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::AuthContext;
use runinator_models::orgs::{
    AddOrgMemberRequest, CreateOrgRequest, OrgContextResponse, OrgMembershipView, OrgRole,
    SwitchOrgRequest, UpdateOrgMemberRequest, UpdateOrgRequest, slugify,
};
use runinator_models::value::Value;
use serde::Serialize;
use uuid::Uuid;

use crate::auth::{AuthConfig, issue_access_token};
use crate::authz;
use crate::models::{ApiError, ApiResponse};
use crate::responses::{api_error, bad_request, not_found};

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

fn require_principal(ctx: &AuthContext) -> Result<Uuid, Reply> {
    ctx.principal_id.ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ApiResponse::ApiError(ApiError::new(
                "this action requires a user principal",
            ))),
        )
    })
}

/// create an organization. the creating user becomes its owner (self-serve signup).
pub(crate) async fn create_org<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<CreateOrgRequest>,
) -> Reply {
    let user_id = match require_principal(&ctx) {
        Ok(id) => id,
        Err(reply) => return reply,
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
pub(crate) async fn list_orgs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = authz::require_admin(&ctx) {
        return reply;
    }
    match db.list_orgs().await {
        Ok(orgs) => ok_value(&orgs),
        Err(err) => api_error(err.to_string()),
    }
}

/// the caller's org memberships, each with their role.
pub(crate) async fn list_my_orgs<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    let user_id = match require_principal(&ctx) {
        Ok(id) => id,
        Err(reply) => return reply,
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
pub(crate) async fn get_org<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_org_member(&ctx, org_id) {
        return reply;
    }
    match db.fetch_org(org_id).await {
        Ok(Some(org)) => ok_value(&org),
        Ok(None) => not_found("organization not found"),
        Err(err) => api_error(err.to_string()),
    }
}

/// rename or (dis|en)able an org (org admin, or platform admin).
pub(crate) async fn update_org<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
    Json(request): Json<UpdateOrgRequest>,
) -> Reply {
    if let Err(reply) = authz::require_org_admin(&ctx, org_id) {
        return reply;
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
pub(crate) async fn delete_org<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_org_role(&ctx, org_id, OrgRole::Owner) {
        return reply;
    }
    match db.delete_org(org_id).await {
        Ok(()) => ok_value(&serde_json::json!({ "deleted": org_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// list an org's members (any member, or platform admin).
pub(crate) async fn list_org_members<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
) -> Reply {
    if let Err(reply) = authz::require_org_member(&ctx, org_id) {
        return reply;
    }
    match db.list_org_members(org_id).await {
        Ok(members) => ok_value(&members),
        Err(err) => api_error(err.to_string()),
    }
}

/// add or re-role a member (org admin, or platform admin).
pub(crate) async fn add_org_member<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(org_id): Path<Uuid>,
    Json(request): Json<AddOrgMemberRequest>,
) -> Reply {
    if let Err(reply) = authz::require_org_admin(&ctx, org_id) {
        return reply;
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
pub(crate) async fn update_org_member<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
    Json(request): Json<UpdateOrgMemberRequest>,
) -> Reply {
    if let Err(reply) = authz::require_org_admin(&ctx, org_id) {
        return reply;
    }
    // guard the last owner: an org must always retain at least one owner.
    if let Err(reply) = guard_last_owner(db.as_ref(), org_id, user_id, request.role).await {
        return reply;
    }
    match db.add_org_member(org_id, user_id, request.role).await {
        Ok(()) => ok_value(&serde_json::json!({ "org_id": org_id, "user_id": user_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// remove a member (org admin, or platform admin).
pub(crate) async fn remove_org_member<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((org_id, user_id)): Path<(Uuid, Uuid)>,
) -> Reply {
    if let Err(reply) = authz::require_org_admin(&ctx, org_id) {
        return reply;
    }
    // removing an owner demotes them out of the org; block if they are the last one.
    if let Err(reply) = guard_last_owner(db.as_ref(), org_id, user_id, OrgRole::Member).await {
        return reply;
    }
    match db.remove_org_member(org_id, user_id).await {
        Ok(()) => ok_value(&serde_json::json!({ "removed": user_id })),
        Err(err) => api_error(err.to_string()),
    }
}

/// switch the active org: re-issue an access token bound to `org_id` and the caller's role there.
pub(crate) async fn switch_org<T: DatabaseImpl>(
    Extension(db): Extension<Arc<T>>,
    Extension(config): Extension<Arc<AuthConfig>>,
    Extension(ctx): Extension<AuthContext>,
    Json(request): Json<SwitchOrgRequest>,
) -> Reply {
    let user_id = match require_principal(&ctx) {
        Ok(id) => id,
        Err(reply) => return reply,
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
        ctx.is_admin,
        Some(request.org_id),
        Some(membership.role),
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
async fn guard_last_owner<T: DatabaseImpl>(
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
