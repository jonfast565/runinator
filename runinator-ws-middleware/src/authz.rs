//! Deny-by-default hierarchical authorization helpers.

use std::collections::HashSet;

use axum::{Json, http::StatusCode};
use chrono::Utc;
use runinator_models::auth::{
    AuthContext, Grant, Permission, PrincipalKind, PrincipalType, ResourceType,
};
use runinator_models::errors::error_code_or_unknown;
use runinator_models::orgs::OrgRole;
use runinator_models::rbac::{
    Action, PlatformRole, Role, ScopeKind, ScopeRef, SystemRole, TeamRole,
};
use runinator_models::revisions::{RevisionAuthor, RevisionSource};
use runinator_models::value::Value;
use runinator_store::{
    RuntimeStore,
    roles::{AuthStore, AutomationStore, RbacStore, ScheduleStore},
};
use uuid::Uuid;

use runinator_ws_core::models::{ApiError, ApiResponse};

type Reply = (StatusCode, Json<ApiResponse>);

/// A compact authorization rejection that handlers can turn back into their standard response.

/// A compact, ready-to-send response used by helper functions that must retain HTTP detail.

/// Convert either an authorization rejection or an already-formed handler reply into a reply.
/// This keeps guard clauses uniform while the compact error crosses crate boundaries.

/// Whether one owned resource may consume another reusable resource. This is evaluated again at
/// run admission so an ownership transfer or revoked grant applies to future runs.
pub async fn resource_can_consume<T: AuthorizationStore>(
    db: &T,
    consumer_type: ResourceType,
    consumer_id: Uuid,
    dependency_type: ResourceType,
    dependency_id: Uuid,
) -> Result<bool, runinator_models::errors::SendableError> {
    runinator_store::resource_access::resource_can_consume(
        db,
        consumer_type,
        consumer_id,
        dependency_type,
        dependency_id,
    )
    .await
}

/// Persistence needed to make one authorization decision, including parent-resource lookups and
/// its best-effort denial audit. This is deliberately narrower than the full database surface:
/// authorization does not need workflow definitions, credentials settings, functions, or task
/// history.

fn forbidden() -> Reply {
    (
        StatusCode::FORBIDDEN,
        Json(ApiResponse::ApiError(ApiError::new(
            "insufficient permission for this resource",
        ))),
    )
}

fn not_found() -> Reply {
    (
        StatusCode::NOT_FOUND,
        Json(ApiResponse::ApiError(ApiError::new("resource not found"))),
    )
}

fn authorization_error() -> Reply {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiResponse::ApiError(ApiError::new(
            "authorization state could not be resolved",
        ))),
    )
}

/// pure predicates over an [`AuthContext`] with no store access. a local trait since `AuthContext`
/// lives in `runinator-models`, which stays free of WS-layer response concepts.

fn system_role_action(role: SystemRole) -> Action {
    match role {
        SystemRole::Engine => Action::EngineOperate,
        SystemRole::Worker => Action::WorkerOperate,
        SystemRole::Waker => Action::WakerOperate,
        SystemRole::Agent => Action::AgentOperate,
        SystemRole::Replica => Action::ReplicaOperate,
    }
}

fn user_owner_allows(action: Action) -> bool {
    matches!(
        action,
        Action::View | Action::Run | Action::Edit | Action::Own | Action::CredentialsManage
    )
}

fn role_allows(role: Role, action: Action) -> bool {
    match role {
        Role::Platform(PlatformRole::Admin) => true,
        Role::Platform(PlatformRole::Operator) => !matches!(
            action,
            Action::Own
                | Action::RolesManage
                | Action::MembersManage
                | Action::CredentialsManage
                | Action::SecretsRead
                | Action::SecretsWrite
                | Action::BillingManage
        ),
        Role::Platform(PlatformRole::Auditor) => matches!(
            action,
            Action::View | Action::AuditRead | Action::DeadLettersRead
        ),
        Role::Platform(PlatformRole::Member) => action == Action::View,
        Role::Organization(OrgRole::Owner) => true,
        Role::Organization(OrgRole::Admin) => action != Action::Own,
        Role::Organization(OrgRole::Operator) => matches!(
            action,
            Action::View
                | Action::Run
                | Action::Edit
                | Action::NodesOperate
                | Action::SchedulesManage
                | Action::NotificationsManage
                | Action::FunctionsManage
                | Action::ConsoleUse
                | Action::CatalogManage
        ),
        Role::Organization(OrgRole::Member) => action == Action::View,
        Role::Team(TeamRole::Owner) => true,
        Role::Team(TeamRole::Admin) => action != Action::Own,
        Role::Team(TeamRole::Operator) => matches!(
            action,
            Action::View
                | Action::Run
                | Action::Edit
                | Action::SchedulesManage
                | Action::NotificationsManage
                | Action::FunctionsManage
                | Action::ConsoleUse
        ),
        Role::Team(TeamRole::Member) => action == Action::View,
        Role::System(_) => false,
    }
}

fn permission_action(permission: Permission) -> Action {
    match permission {
        Permission::View => Action::View,
        Permission::Run => Action::Run,
        Permission::Edit => Action::Edit,
        Permission::Own => Action::Own,
    }
}

fn ceiling_allows(ctx: &AuthContext, permission: Permission) -> bool {
    ctx.action_ceiling.is_empty() || ctx.action_ceiling.contains(&permission_action(permission))
}

/// resource-visibility checks that need both a store handle and the caller's identity. `db` and
/// `ctx` are genuinely invariant across every method here (unlike a graph cursor's node/run, which
/// varies per call), so both belong on `self` rather than threaded through each call individually.

pub fn record_workflow_run_id(record: &Value) -> Option<Uuid> {
    record
        .get("workflow_run_id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
}

#[cfg(test)]
#[path = "authz_policy_tests.rs"]
mod policy_tests;

mod authorization_denied;
pub use authorization_denied::AuthorizationDenied;

mod guard_error;
pub use guard_error::GuardError;

mod into_reply;
pub use into_reply::IntoReply;

mod authorization_store;
pub use authorization_store::AuthorizationStore;

mod auth_context_ext;
pub use auth_context_ext::AuthContextExt;

mod authz_checker;
pub use authz_checker::AuthzChecker;
