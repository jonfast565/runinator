//! Canonical hierarchical RBAC administration and generic resource ACL endpoints.

use std::sync::Arc;

use axum::{Extension, Json, extract::Path, http::StatusCode};
use chrono::Utc;
use runinator_models::{
    auth::{
        AuthContext, CreateGrantRequest, Grant, Permission, PrincipalKind, PrincipalType,
        ResourceType,
    },
    orgs::OrgRole,
    rbac::{Action, PlatformRole, Role, ScopeKind, ScopeRef, TeamRole},
    validation::{SHORT_TEXT_MAX, Validate, ValidationError, required_text},
    value::Value,
};
use runinator_ws_core::{
    ValidatedJson,
    models::{ApiError, ApiResponse},
    openapi::docs::{EndpointDoc, EndpointPolicy, Example, endpoint_with_policy, json_body},
    responses::{api_error, bad_request, not_found, task_response_success},
};
use runinator_ws_middleware::authz::AuthorizationStore;
use runinator_ws_middleware::authz::{AuthContextExt, AuthzChecker};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

type Reply = (StatusCode, Json<ApiResponse>);

async fn audit_change<T: AuthorizationStore>(
    db: &T,
    ctx: &AuthContext,
    action: &str,
    resource_type: Option<&str>,
    resource_id: Option<Uuid>,
    detail: &str,
) {
    crate::audit::record_audit(
        db,
        ctx.principal_id,
        ctx.actor_kind(),
        action,
        crate::audit::AuditOutcome::Success,
        resource_type,
        resource_id,
        Some(detail),
    )
    .await;
}

#[derive(Debug, Deserialize)]
pub struct SetRoleRequest {
    pub role: Role,
}

#[derive(Debug, Deserialize)]
pub struct TransferOwnerRequest {
    pub owner: ScopeRef,
}

#[derive(Debug, Deserialize)]
pub struct CreateServiceAccountRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateServiceAccountRequest {
    pub disabled: bool,
}

impl Validate for SetRoleRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for TransferOwnerRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

impl Validate for CreateServiceAccountRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        required_text("name", &self.name, SHORT_TEXT_MAX)
    }
}

impl Validate for UpdateServiceAccountRequest {
    fn validate(&self) -> Result<(), ValidationError> {
        Ok(())
    }
}

#[derive(Serialize)]
struct AuthzCatalog {
    actions: &'static [Action],
    platform_roles: [PlatformRole; 4],
    organization_roles: [OrgRole; 4],
    team_roles: [TeamRole; 4],
    resource_types: [ResourceType; 9],
}

fn ok<T: Serialize>(value: &T) -> Reply {
    match serde_json::to_value(value) {
        Ok(value) => (
            StatusCode::OK,
            Json(ApiResponse::JsonValue(Value::from(value))),
        ),
        Err(err) => api_error(err.to_string()),
    }
}

fn forbidden(message: &str) -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(ApiError::new(message))),
    )
}
fn parse_scope(kind: &str, id: &str) -> Result<ScopeRef, Reply> {
    let Some(kind) = ScopeKind::from_str_lossy(kind) else {
        return Err(bad_request("unknown scope kind"));
    };
    let id = if kind == ScopeKind::Platform {
        if id != "platform" {
            return Err(bad_request("platform scope id must be 'platform'"));
        }
        None
    } else {
        Some(
            id.parse::<Uuid>()
                .map_err(|_| bad_request("invalid scope id"))?,
        )
    };
    ScopeRef::new(kind, id).ok_or_else(|| bad_request("invalid scope"))
}
fn parse_principal(kind: &str) -> Result<PrincipalKind, Reply> {
    match PrincipalKind::from_str_lossy(kind) {
        Some(PrincipalKind::User) => Ok(PrincipalKind::User),
        Some(PrincipalKind::Service) => Ok(PrincipalKind::Service),
        _ => Err(bad_request(
            "role assignments support user and service principals only",
        )),
    }
}

fn role_matches_scope(role: Role, scope: ScopeRef) -> bool {
    matches!(
        (role, scope.kind),
        (Role::Platform(_), ScopeKind::Platform)
            | (Role::Organization(_), ScopeKind::Organization)
            | (Role::Team(_), ScopeKind::Team)
    )
}

async fn principal_exists<T: AuthorizationStore>(
    db: &T,
    kind: PrincipalKind,
    id: Uuid,
) -> Result<bool, Reply> {
    match kind {
        PrincipalKind::User => db
            .fetch_user(id)
            .await
            .map(|user| user.is_some_and(|user| !user.disabled))
            .map_err(|err| api_error(err.to_string())),
        PrincipalKind::Service => db
            .fetch_service_account(id)
            .await
            .map(|account| account.is_some_and(|account| !account.disabled))
            .map_err(|err| api_error(err.to_string())),
    }
}

async fn authorize_scope_with_ancestry<T: AuthorizationStore>(
    db: &T,
    ctx: &AuthContext,
    action: Action,
    scope: ScopeRef,
) -> Result<bool, Reply> {
    if ctx.authorize_scope(action, scope) {
        return Ok(true);
    }
    if scope.kind != ScopeKind::Team {
        return Ok(false);
    }
    let Some(team_id) = scope.id else {
        return Ok(false);
    };
    let Some(team) = db
        .fetch_team(team_id)
        .await
        .map_err(|err| api_error(err.to_string()))?
    else {
        return Ok(false);
    };
    Ok(ctx.authorize_scope(action, team.scope))
}

async fn can_assign<T: AuthorizationStore>(
    db: &T,
    ctx: &AuthContext,
    scope: ScopeRef,
    role: Role,
) -> Result<bool, Reply> {
    if ctx.is_platform_admin() {
        return Ok(true);
    }
    if matches!(
        role,
        Role::Organization(OrgRole::Owner) | Role::Team(TeamRole::Owner)
    ) {
        return authorize_scope_with_ancestry(db, ctx, Action::Own, scope).await;
    }
    if !authorize_scope_with_ancestry(db, ctx, Action::RolesManage, scope).await? {
        return Ok(false);
    }
    Ok(true)
}

pub async fn catalog(Extension(_ctx): Extension<AuthContext>) -> Reply {
    ok(&AuthzCatalog {
        actions: Action::ALL,
        platform_roles: [
            PlatformRole::Admin,
            PlatformRole::Operator,
            PlatformRole::Auditor,
            PlatformRole::Member,
        ],
        organization_roles: [
            OrgRole::Owner,
            OrgRole::Admin,
            OrgRole::Operator,
            OrgRole::Member,
        ],
        team_roles: [
            TeamRole::Owner,
            TeamRole::Admin,
            TeamRole::Operator,
            TeamRole::Member,
        ],
        resource_types: [
            ResourceType::Workflow,
            ResourceType::Pipeline,
            ResourceType::FunctionPackage,
            ResourceType::ConsoleSession,
            ResourceType::Setting,
            ResourceType::ExecutionProfile,
            ResourceType::OrchestrationAdapter,
            ResourceType::LibraryFile,
            ResourceType::NotificationPolicy,
        ],
    })
}

pub async fn list_assignments<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, id)): Path<(String, String)>,
) -> Reply {
    let scope = match parse_scope(&kind, &id) {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let authorized =
        match authorize_scope_with_ancestry(db.as_ref(), &ctx, Action::RolesManage, scope).await {
            Ok(true) => true,
            Ok(false) => {
                match authorize_scope_with_ancestry(db.as_ref(), &ctx, Action::MembersManage, scope)
                    .await
                {
                    Ok(value) => value,
                    Err(reply) => return reply,
                }
            }
            Err(reply) => return reply,
        };
    if !authorized {
        return forbidden("role administration is not permitted in this scope");
    }
    match db.list_scope_role_assignments(scope).await {
        Ok(rows) => ok(&rows),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn set_assignment<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((scope_kind, scope_id, principal_kind, principal_id)): Path<(
        String,
        String,
        String,
        Uuid,
    )>,
    ValidatedJson(request): ValidatedJson<SetRoleRequest>,
) -> Reply {
    let scope = match parse_scope(&scope_kind, &scope_id) {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let principal_kind = match parse_principal(&principal_kind) {
        Ok(kind) => kind,
        Err(reply) => return reply,
    };
    if !role_matches_scope(request.role, scope) {
        return bad_request("role kind does not match scope kind");
    }
    match can_assign(db.as_ref(), &ctx, scope, request.role).await {
        Ok(true) => {}
        Ok(false) => return forbidden("cannot delegate this role"),
        Err(reply) => return reply,
    }
    match principal_exists(db.as_ref(), principal_kind, principal_id).await {
        Ok(true) => {}
        Ok(false) => return bad_request("principal does not exist or is disabled"),
        Err(reply) => return reply,
    }
    match db
        .upsert_role_assignment(
            principal_kind,
            principal_id,
            scope,
            request.role,
            ctx.principal_id,
        )
        .await
    {
        Ok(row) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.role.set",
                Some(scope.kind.as_str()),
                scope.id,
                request.role.as_str(),
            )
            .await;
            ok(&row)
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_assignment<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((scope_kind, scope_id, principal_kind, principal_id)): Path<(
        String,
        String,
        String,
        Uuid,
    )>,
) -> Reply {
    let scope = match parse_scope(&scope_kind, &scope_id) {
        Ok(scope) => scope,
        Err(reply) => return reply,
    };
    let principal_kind = match parse_principal(&principal_kind) {
        Ok(kind) => kind,
        Err(reply) => return reply,
    };
    match authorize_scope_with_ancestry(db.as_ref(), &ctx, Action::RolesManage, scope).await {
        Ok(true) => {}
        Ok(false) => return forbidden("role administration is not permitted in this scope"),
        Err(reply) => return reply,
    }
    let assignments = match db.list_scope_role_assignments(scope).await {
        Ok(rows) => rows,
        Err(err) => return api_error(err.to_string()),
    };
    let target = assignments
        .iter()
        .find(|row| row.principal_kind == principal_kind && row.principal_id == principal_id);
    let is_protected_owner = target.is_some_and(|row| {
        matches!(
            row.role,
            Role::Platform(PlatformRole::Admin)
                | Role::Organization(OrgRole::Owner)
                | Role::Team(TeamRole::Owner)
        )
    });
    if is_protected_owner {
        let owners = assignments
            .iter()
            .filter(|row| row.role == target.unwrap().role)
            .count();
        if owners <= 1 {
            return forbidden("the last administrator or owner cannot be removed");
        }
    }
    match db
        .delete_role_assignment(principal_kind, principal_id, scope)
        .await
    {
        Ok(()) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.role.delete",
                Some(scope.kind.as_str()),
                scope.id,
                &principal_id.to_string(),
            )
            .await;
            task_response_success("Role assignment removed")
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_resource_grants<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, resource_id)): Path<(String, Uuid)>,
) -> Reply {
    let Some(resource_type) = ResourceType::from_str_lossy(&kind) else {
        return bad_request("unknown resource type");
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(resource_type, resource_id, Permission::Own)
        .await
    {
        return reply;
    }
    match db.list_grants(kind, resource_id).await {
        Ok(grants) => ok(&grants),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn get_resource_owner<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, resource_id)): Path<(String, Uuid)>,
) -> Reply {
    let Some(resource_type) = ResourceType::from_str_lossy(&kind) else {
        return bad_request("unknown resource type");
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(resource_type, resource_id, Permission::View)
        .await
    {
        return reply;
    }
    match db
        .fetch_resource_ownership(resource_type, resource_id)
        .await
    {
        Ok(Some(ownership)) => ok(&ownership),
        Ok(None) => not_found("Resource not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_resource_grant<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, resource_id)): Path<(String, Uuid)>,
    ValidatedJson(request): ValidatedJson<CreateGrantRequest>,
) -> Reply {
    let Some(resource_type) = ResourceType::from_str_lossy(&kind) else {
        return bad_request("unknown resource type");
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(resource_type, resource_id, Permission::Own)
        .await
    {
        return reply;
    }
    let ownership = match db
        .fetch_resource_ownership(resource_type, resource_id)
        .await
    {
        Ok(Some(ownership)) => ownership,
        Ok(None) => return not_found("Resource not found"),
        Err(err) => return api_error(err.to_string()),
    };
    let valid = match request.principal_type {
        PrincipalType::User => {
            let user_exists = match db.fetch_user(request.principal_id).await {
                Ok(user) => user.is_some_and(|user| !user.disabled),
                Err(err) => return api_error(err.to_string()),
            };
            if !user_exists {
                Ok(false)
            } else if ownership.tenant.kind == ScopeKind::Organization {
                db.list_principal_role_assignments(PrincipalKind::User, request.principal_id)
                    .await
                    .map(|assignments| {
                        assignments
                            .iter()
                            .any(|assignment| assignment.scope == ownership.tenant)
                    })
            } else {
                Ok(true)
            }
        }
        PrincipalType::Team => db
            .fetch_team(request.principal_id)
            .await
            .map(|team| team.is_some_and(|team| team.scope == ownership.tenant)),
    };
    match valid {
        Ok(true) => {}
        Ok(false) => return bad_request("grant principal does not exist or is disabled"),
        Err(err) => return api_error(err.to_string()),
    }
    let grant = Grant {
        id: None,
        resource_type,
        resource_id,
        principal_type: request.principal_type,
        principal_id: request.principal_id,
        permission: request.permission,
        created_at: Utc::now(),
    };
    match db.create_grant(grant).await {
        Ok(grant) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.grant.create",
                Some(resource_type.as_str()),
                Some(resource_id),
                request.permission.as_str(),
            )
            .await;
            ok(&grant)
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn delete_resource_grant<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, resource_id, grant_id)): Path<(String, Uuid, Uuid)>,
) -> Reply {
    let Some(resource_type) = ResourceType::from_str_lossy(&kind) else {
        return bad_request("unknown resource type");
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(resource_type, resource_id, Permission::Own)
        .await
    {
        return reply;
    }
    match db
        .revoke_scoped_grant(resource_type, resource_id, grant_id)
        .await
    {
        Ok(true) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.grant.delete",
                Some(resource_type.as_str()),
                Some(resource_id),
                &grant_id.to_string(),
            )
            .await;
            task_response_success("Grant removed")
        }
        Ok(false) => not_found("Grant not found"),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn transfer_resource<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path((kind, resource_id)): Path<(String, Uuid)>,
    ValidatedJson(request): ValidatedJson<TransferOwnerRequest>,
) -> Reply {
    let Some(resource_type) = ResourceType::from_str_lossy(&kind) else {
        return bad_request("unknown resource type");
    };
    if let Err(reply) = AuthzChecker::new(db.as_ref(), &ctx)
        .require_resource(resource_type, resource_id, Permission::Own)
        .await
    {
        return reply;
    }
    let ownership = match db
        .fetch_resource_ownership(resource_type, resource_id)
        .await
    {
        Ok(Some(ownership)) => ownership,
        Ok(None) => return not_found("Resource not found"),
        Err(err) => return api_error(err.to_string()),
    };
    if ownership.tenant.kind == ScopeKind::Platform && request.owner.kind != ScopeKind::Platform {
        return bad_request("platform resources must remain platform-owned");
    }
    if request.owner.kind == ScopeKind::Platform {
        if !ctx.is_platform_admin() {
            return forbidden("only platform administrators can assign platform ownership");
        }
        if !matches!(
            resource_type,
            ResourceType::Setting
                | ResourceType::ExecutionProfile
                | ResourceType::NotificationPolicy
        ) {
            return bad_request("this resource type cannot be platform-owned");
        }
    }
    if request.owner.kind != ScopeKind::User {
        match authorize_scope_with_ancestry(db.as_ref(), &ctx, Action::Own, request.owner).await {
            Ok(true) => {}
            Ok(false) => {
                return forbidden("cannot transfer a resource into a scope you do not own");
            }
            Err(reply) => return reply,
        }
    }
    let target_team = if request.owner.kind == ScopeKind::Team {
        match request.owner.id {
            Some(id) => match db.fetch_team(id).await {
                Ok(team) => team,
                Err(err) => return api_error(err.to_string()),
            },
            None => return bad_request("invalid team owner scope"),
        }
    } else {
        None
    };
    let target_exists = match (request.owner.kind, request.owner.id) {
        (ScopeKind::Platform, None) => Ok(true),
        (ScopeKind::User, Some(id)) => db
            .fetch_user(id)
            .await
            .map(|row| row.is_some_and(|user| !user.disabled)),
        (ScopeKind::Organization, Some(id)) => db
            .fetch_org(id)
            .await
            .map(|row| row.is_some_and(|org| !org.disabled)),
        (ScopeKind::Team, Some(_)) => Ok(target_team.is_some()),
        _ => return bad_request("invalid owner scope"),
    };
    match target_exists {
        Ok(true) => {}
        Ok(false) => return bad_request("target owner scope does not exist or is disabled"),
        Err(err) => return api_error(err.to_string()),
    }
    if ownership.tenant.kind == ScopeKind::Organization {
        let target_tenant = match request.owner.kind {
            ScopeKind::Organization => Some(request.owner),
            ScopeKind::Team => target_team.as_ref().map(|team| team.scope),
            ScopeKind::User => None,
            ScopeKind::Platform => Some(ScopeRef::PLATFORM),
        };
        if target_tenant.is_some_and(|tenant| tenant != ownership.tenant) {
            return bad_request("target owner is outside the resource organization");
        }
        if request.owner.kind == ScopeKind::User {
            let target_id = request.owner.id.expect("validated user scope");
            let assignments = match db
                .list_principal_role_assignments(PrincipalKind::User, target_id)
                .await
            {
                Ok(assignments) => assignments,
                Err(err) => return api_error(err.to_string()),
            };
            if !assignments
                .iter()
                .any(|assignment| assignment.scope == ownership.tenant)
            {
                return bad_request("target user is not a member of the resource organization");
            }
        }
    }
    match db
        .transfer_resource_ownership(resource_type, resource_id, request.owner)
        .await
    {
        Ok(owner) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.owner.transfer",
                Some(resource_type.as_str()),
                Some(resource_id),
                request.owner.kind.as_str(),
            )
            .await;
            ok(&owner)
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn list_service_accounts<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ScopeRef::PLATFORM) {
        return reply;
    }
    match db.list_service_accounts().await {
        Ok(rows) => ok(&rows),
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn create_service_account<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    ValidatedJson(request): ValidatedJson<CreateServiceAccountRequest>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ScopeRef::PLATFORM) {
        return reply;
    }
    if request.name.trim().is_empty() {
        return bad_request("service account name must not be empty");
    }
    match db
        .create_service_account(request.name, ctx.principal_id)
        .await
    {
        Ok(account) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.service_account.create",
                Some("service_account"),
                Some(account.id),
                &account.name,
            )
            .await;
            ok(&account)
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub async fn update_service_account<T: AuthorizationStore>(
    Extension(db): Extension<Arc<T>>,
    Extension(ctx): Extension<AuthContext>,
    Path(id): Path<Uuid>,
    ValidatedJson(request): ValidatedJson<UpdateServiceAccountRequest>,
) -> Reply {
    if let Err(reply) = ctx.require_scope_action(Action::CredentialsManage, ScopeRef::PLATFORM) {
        return reply;
    }
    match db.set_service_account_disabled(id, request.disabled).await {
        Ok(account) => {
            audit_change(
                db.as_ref(),
                &ctx,
                "authz.service_account.update",
                Some("service_account"),
                Some(account.id),
                if account.disabled {
                    "disabled"
                } else {
                    "enabled"
                },
            )
            .await;
            ok(&account)
        }
        Err(err) => api_error(err.to_string()),
    }
}

pub fn routes<T: AuthorizationStore>(pool: Arc<T>) -> axum::Router {
    use axum::routing::{delete, get, patch, put};
    axum::Router::new()
        .route("/authz/catalog", get(catalog))
        .route(
            "/authz/scopes/{kind}/{id}/assignments",
            get(list_assignments::<T>),
        )
        .route(
            "/authz/scopes/{kind}/{id}/assignments/{principal_kind}/{principal_id}",
            put(set_assignment::<T>).delete(delete_assignment::<T>),
        )
        .route(
            "/authz/resources/{kind}/{resource_id}/grants",
            get(list_resource_grants::<T>).post(create_resource_grant::<T>),
        )
        .route(
            "/authz/resources/{kind}/{resource_id}/grants/{grant_id}",
            delete(delete_resource_grant::<T>),
        )
        .route(
            "/authz/resources/{kind}/{resource_id}/owner",
            get(get_resource_owner::<T>).post(transfer_resource::<T>),
        )
        .route(
            "/service_accounts",
            get(list_service_accounts::<T>).post(create_service_account::<T>),
        )
        .route("/service_accounts/{id}", patch(update_service_account::<T>))
        .layer(Extension(pool))
}

pub const DOCS: &[EndpointDoc] = &[
    endpoint_with_policy!(
        "get",
        "/authz/catalog",
        "Authorization",
        "Read authorization catalog",
        "Returns canonical fixed roles, actions, scopes, and resource types.",
        EndpointPolicy::Authenticated,
        None,
        &[],
        200,
        "authorization catalog",
        Example::None,
    ),
    endpoint_with_policy!(
        "get",
        "/authz/scopes/{kind}/{id}/assignments",
        "Authorization",
        "List role assignments",
        "Lists assignments in an authorized scope.",
        EndpointPolicy::ScopedAction(Action::RolesManage),
        None,
        &[],
        200,
        "role assignments",
        Example::None,
    ),
    endpoint_with_policy!(
        "put",
        "/authz/scopes/{kind}/{id}/assignments/{principal_kind}/{principal_id}",
        "Authorization",
        "Set role assignment",
        "Creates or updates a scoped role assignment.",
        EndpointPolicy::ScopedAction(Action::RolesManage),
        json_body("Role assignment.", Example::None),
        &[],
        200,
        "role assignment",
        Example::None,
    ),
    endpoint_with_policy!(
        "delete",
        "/authz/scopes/{kind}/{id}/assignments/{principal_kind}/{principal_id}",
        "Authorization",
        "Delete role assignment",
        "Removes a scoped role assignment while protecting the last owner.",
        EndpointPolicy::ScopedAction(Action::RolesManage),
        None,
        &[],
        200,
        "assignment removed",
        Example::None,
    ),
    endpoint_with_policy!(
        "get",
        "/authz/resources/{kind}/{resource_id}/grants",
        "Authorization",
        "List resource grants",
        "Lists generic grants after resolving stored ownership.",
        EndpointPolicy::AnyResourceAction(Action::Own),
        None,
        &[],
        200,
        "resource grants",
        Example::None,
    ),
    endpoint_with_policy!(
        "get",
        "/authz/resources/{kind}/{resource_id}/owner",
        "Authorization",
        "Read resource owner",
        "Returns the effective generic owner after requiring view access to the resource.",
        EndpointPolicy::AnyResourceAction(Action::View),
        None,
        &[],
        200,
        "resource ownership",
        Example::None,
    ),
    endpoint_with_policy!(
        "post",
        "/authz/resources/{kind}/{resource_id}/grants",
        "Authorization",
        "Create resource grant",
        "Creates a generic resource grant.",
        EndpointPolicy::AnyResourceAction(Action::Own),
        json_body("Grant.", Example::None),
        &[],
        200,
        "resource grant",
        Example::None,
    ),
    endpoint_with_policy!(
        "delete",
        "/authz/resources/{kind}/{resource_id}/grants/{grant_id}",
        "Authorization",
        "Delete resource grant",
        "Deletes only a grant belonging to the authorized resource.",
        EndpointPolicy::AnyResourceAction(Action::Own),
        None,
        &[],
        200,
        "grant removed",
        Example::None,
    ),
    endpoint_with_policy!(
        "post",
        "/authz/resources/{kind}/{resource_id}/owner",
        "Authorization",
        "Transfer resource owner",
        "Transfers the generic stored owner scope.",
        EndpointPolicy::AnyResourceAction(Action::Own),
        json_body("Owner scope.", Example::None),
        &[],
        200,
        "resource ownership",
        Example::None,
    ),
    endpoint_with_policy!(
        "get",
        "/service_accounts",
        "Authorization",
        "List service accounts",
        "Lists first-class machine principals.",
        EndpointPolicy::ScopedAction(Action::CredentialsManage),
        None,
        &[],
        200,
        "service accounts",
        Example::None,
    ),
    endpoint_with_policy!(
        "post",
        "/service_accounts",
        "Authorization",
        "Create service account",
        "Creates a first-class machine principal.",
        EndpointPolicy::ScopedAction(Action::CredentialsManage),
        json_body("Service account.", Example::None),
        &[],
        200,
        "service account",
        Example::None,
    ),
    endpoint_with_policy!(
        "patch",
        "/service_accounts/{id}",
        "Authorization",
        "Update service account",
        "Enables or disables a machine principal.",
        EndpointPolicy::ScopedAction(Action::CredentialsManage),
        json_body("Service account state.", Example::None),
        &[],
        200,
        "service account",
        Example::None,
    ),
];
