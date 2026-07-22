//! resource-based authorization helpers (phase 2). admins (and the synthetic admin used when auth is
//! disabled) implicitly own everything, so these short-circuit and existing behavior is unchanged
//! until grants exist.

use std::collections::HashSet;

use axum::{Json, http::StatusCode};
use chrono::Utc;
use runinator_database::interfaces::DatabaseImpl;
use runinator_models::auth::{
    AuthContext, Grant, Permission, PrincipalKind, PrincipalType, ResourceType,
};
use runinator_models::capabilities::Capability;
use runinator_models::orgs::OrgRole;
use runinator_models::value::Value;
use uuid::Uuid;

use crate::models::{ApiError, ApiResponse};

type Reply = (StatusCode, Json<ApiResponse>);

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

fn workflow_kind() -> String {
    ResourceType::Workflow.as_str().to_string()
}

fn pipeline_kind() -> String {
    ResourceType::Pipeline.as_str().to_string()
}

pub fn require_admin(ctx: &AuthContext) -> Result<(), Reply> {
    if ctx.is_admin {
        Ok(())
    } else {
        Err(forbidden())
    }
}

pub fn require_service_or_admin(ctx: &AuthContext) -> Result<(), Reply> {
    if ctx.is_admin || matches!(ctx.kind, PrincipalKind::Service) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

/// the capability set a caller holds. this is the single documented mapping of who-holds-what:
/// platform admins (including the synthetic admin used when auth is disabled) hold every capability;
/// admins of the caller's active org hold the org-scoped capabilities; ordinary members hold none.
/// returned on `/auth/me` so the command center gates against the same truth the handlers enforce.
pub fn capabilities_for(ctx: &AuthContext) -> HashSet<Capability> {
    if ctx.is_admin {
        return Capability::ALL.iter().copied().collect();
    }
    match ctx.org_role {
        Some(role) if role.allows(OrgRole::Admin) => {
            Capability::ORG_ADMIN.iter().copied().collect()
        }
        _ => HashSet::new(),
    }
}

/// gate an action on a named capability, else a 403 reply. platform-scoped capabilities pass only for
/// platform admins; org-scoped capabilities pass for admins of the active org (see `capabilities_for`).
pub fn require_capability(ctx: &AuthContext, cap: Capability) -> Result<(), Reply> {
    if capabilities_for(ctx).contains(&cap) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

/// gate an org-scoped action: platform admins transcend org roles; otherwise the caller's active org
/// must match `org_id` and their role must be at least `min`.
pub fn require_org_role(ctx: &AuthContext, org_id: Uuid, min: OrgRole) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match (ctx.org_id, ctx.org_role) {
        (Some(active), Some(role)) if active == org_id && role.allows(min) => Ok(()),
        _ => Err(forbidden()),
    }
}

/// require org-admin (or platform admin) for `org_id`.
pub fn require_org_admin(ctx: &AuthContext, org_id: Uuid) -> Result<(), Reply> {
    require_org_role(ctx, org_id, OrgRole::Admin)
}

/// require any membership (or platform admin) in `org_id`.
pub fn require_org_member(ctx: &AuthContext, org_id: Uuid) -> Result<(), Reply> {
    require_org_role(ctx, org_id, OrgRole::Member)
}

/// whether the caller may see a resource owned by `resource_org`. platform admins see everything;
/// `None` (platform-global / unassigned) is a shared library visible to all; otherwise the caller's
/// active org must match. this composes with, and is orthogonal to, per-resource grants.
pub fn org_visible(ctx: &AuthContext, resource_org: Option<Uuid>) -> bool {
    if ctx.is_admin {
        return true;
    }
    match resource_org {
        None => true,
        Some(org) => ctx.org_id == Some(org),
    }
}

/// the caller's effective permission on a workflow, or `None` when they have no access.
pub async fn workflow_permission<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    workflow_id: Uuid,
) -> Option<Permission> {
    if ctx.is_admin {
        return Some(Permission::Own);
    }
    let user_id = ctx.principal_id?;
    let team_ids = db.list_user_team_ids(user_id).await.unwrap_or_default();
    let grants = db.list_grants(workflow_kind(), workflow_id).await.ok()?;
    grants
        .into_iter()
        .filter(|grant| match grant.principal_type {
            PrincipalType::User => grant.principal_id == user_id,
            PrincipalType::Team => team_ids.contains(&grant.principal_id),
        })
        .map(|grant| grant.permission)
        .max()
}

/// require at least `needed` permission on the workflow, else a 403 reply.
pub async fn require_workflow<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    workflow_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match workflow_permission(db, ctx, workflow_id).await {
        Some(permission) if permission.allows(needed) => Ok(()),
        _ => {
            audit_denied(db, ctx, workflow_id, needed).await;
            Err(forbidden())
        }
    }
}

/// the audit `actor_kind` string for a principal.
fn actor_kind(ctx: &AuthContext) -> &'static str {
    match ctx.kind {
        PrincipalKind::User => "user",
        PrincipalKind::Service => "service",
    }
}

/// record an authorization denial against a workflow resource.
async fn audit_denied<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    workflow_id: Uuid,
    needed: Permission,
) {
    crate::audit::record_audit(
        db,
        ctx.principal_id,
        actor_kind(ctx),
        "authz.denied",
        crate::audit::AuditOutcome::Denied,
        Some(ResourceType::Workflow.as_str()),
        Some(workflow_id),
        Some(&format!("missing {:?} permission", needed)),
    )
    .await;
}

/// the workflow ids the caller can see, or `None` meaning "all" (admin / auth disabled).
pub async fn visible_workflow_ids<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
) -> Option<HashSet<Uuid>> {
    if ctx.is_admin {
        return None;
    }
    let mut ids = HashSet::new();
    // every workflow owned by the caller's active org is visible to its members, so org membership
    // grants run visibility without needing an explicit per-workflow grant. this is what isolates
    // runs by org: a caller only ever sees runs whose workflow is org-owned or explicitly granted.
    if let Some(org_id) = ctx.org_id {
        if let Ok(org_ids) = db.fetch_workflow_ids_for_org(org_id).await {
            ids.extend(org_ids);
        }
    }
    let Some(user_id) = ctx.principal_id else {
        return Some(ids);
    };
    if let Ok(grants) = db.list_user_grants(workflow_kind(), user_id).await {
        ids.extend(grants.into_iter().map(|grant| grant.resource_id));
    }
    if let Ok(team_ids) = db.list_user_team_ids(user_id).await {
        for team_id in team_ids {
            if let Ok(grants) = db.list_team_grants(workflow_kind(), team_id).await {
                ids.extend(grants.into_iter().map(|grant| grant.resource_id));
            }
        }
    }
    Some(ids)
}

/// stamp the creator as `own` on a freshly created workflow. a no-op for service/admin principals
/// without a user id (nothing to own it).
pub async fn grant_owner<T: DatabaseImpl>(db: &T, ctx: &AuthContext, workflow_id: Uuid) {
    let Some(user_id) = ctx.principal_id else {
        return;
    };
    let grant = Grant {
        id: None,
        resource_type: ResourceType::Workflow,
        resource_id: workflow_id,
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::Own,
        created_at: Utc::now(),
    };
    let _ = db.create_grant(grant).await;
}

/// the caller's effective permission on a pipeline, or `None` when they have no access.
pub async fn pipeline_permission<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    pipeline_id: Uuid,
) -> Option<Permission> {
    if ctx.is_admin {
        return Some(Permission::Own);
    }
    let user_id = ctx.principal_id?;
    let team_ids = db.list_user_team_ids(user_id).await.unwrap_or_default();
    let grants = db.list_grants(pipeline_kind(), pipeline_id).await.ok()?;
    grants
        .into_iter()
        .filter(|grant| match grant.principal_type {
            PrincipalType::User => grant.principal_id == user_id,
            PrincipalType::Team => team_ids.contains(&grant.principal_id),
        })
        .map(|grant| grant.permission)
        .max()
}

/// require at least `needed` permission on the pipeline, else a 403 reply.
pub async fn require_pipeline<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    pipeline_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match pipeline_permission(db, ctx, pipeline_id).await {
        Some(permission) if permission.allows(needed) => Ok(()),
        _ => Err(forbidden()),
    }
}

/// the pipeline ids the caller can see, or `None` meaning "all" (admin / auth disabled).
pub async fn visible_pipeline_ids<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
) -> Option<HashSet<Uuid>> {
    if ctx.is_admin {
        return None;
    }
    let mut ids = HashSet::new();
    // every pipeline owned by the caller's active org is visible to its members.
    if let Some(org_id) = ctx.org_id {
        if let Ok(org_ids) = db.fetch_pipeline_ids_for_org(org_id).await {
            ids.extend(org_ids);
        }
    }
    let Some(user_id) = ctx.principal_id else {
        return Some(ids);
    };
    if let Ok(grants) = db.list_user_grants(pipeline_kind(), user_id).await {
        ids.extend(grants.into_iter().map(|grant| grant.resource_id));
    }
    if let Ok(team_ids) = db.list_user_team_ids(user_id).await {
        for team_id in team_ids {
            if let Ok(grants) = db.list_team_grants(pipeline_kind(), team_id).await {
                ids.extend(grants.into_iter().map(|grant| grant.resource_id));
            }
        }
    }
    Some(ids)
}

/// stamp the creator as `own` on a freshly created pipeline. a no-op for service/admin principals
/// without a user id (nothing to own it).
pub async fn grant_pipeline_owner<T: DatabaseImpl>(db: &T, ctx: &AuthContext, pipeline_id: Uuid) {
    let Some(user_id) = ctx.principal_id else {
        return;
    };
    let grant = Grant {
        id: None,
        resource_type: ResourceType::Pipeline,
        resource_id: pipeline_id,
        principal_type: PrincipalType::User,
        principal_id: user_id,
        permission: Permission::Own,
        created_at: Utc::now(),
    };
    let _ = db.create_grant(grant).await;
}

/// convenience for run-scoped handlers: gate by the parent workflow's permission.
pub async fn require_run_workflow<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    workflow_run_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match crate::repository::fetch_workflow_run(db, workflow_run_id).await {
        Ok(Some((run, _))) => require_workflow(db, ctx, run.workflow_id, needed).await,
        _ => Err(not_found()),
    }
}

pub async fn require_trigger_workflow<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    trigger_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match db.fetch_workflow_trigger(trigger_id).await {
        Ok(Some(trigger)) => require_workflow(db, ctx, trigger.workflow_id, needed).await,
        _ => Err(not_found()),
    }
}

/// gate a pipeline-trigger handler by its owning pipeline's permission.
pub async fn require_pipeline_trigger<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    trigger_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match db.fetch_pipeline_trigger(trigger_id).await {
        Ok(Some(trigger)) => require_pipeline(db, ctx, trigger.pipeline_id, needed).await,
        _ => Err(not_found()),
    }
}

/// gate a pipeline-run handler by its owning pipeline's permission.
pub async fn require_pipeline_run<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    pipeline_run_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    match db.fetch_pipeline_run(pipeline_run_id).await {
        Ok(Some(run)) => require_pipeline(db, ctx, run.pipeline_id, needed).await,
        _ => Err(not_found()),
    }
}

pub async fn require_node_run_workflow<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    node_run_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    let workflow_run_id = match db.fetch_workflow_node_run(node_run_id).await {
        Ok(Some(node_run)) => node_run.workflow_run_id,
        _ => return Err(not_found()),
    };
    require_run_workflow(db, ctx, workflow_run_id, needed).await
}

pub async fn require_gate_workflow<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    gate_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    let workflow_run_id = match db.fetch_gate(gate_id).await {
        Ok(Some(record)) => record_workflow_run_id(&record),
        _ => None,
    };
    match workflow_run_id {
        Some(workflow_run_id) => require_run_workflow(db, ctx, workflow_run_id, needed).await,
        None => Err(not_found()),
    }
}

pub async fn require_automation_record_workflow<T: DatabaseImpl>(
    db: &T,
    ctx: &AuthContext,
    record_type: &str,
    record_id: Uuid,
    needed: Permission,
) -> Result<(), Reply> {
    if ctx.is_admin {
        return Ok(());
    }
    let workflow_run_id = match db
        .fetch_automation_record(record_type.to_string(), record_id)
        .await
    {
        Ok(Some(record)) => record_workflow_run_id(&record),
        _ => None,
    };
    match workflow_run_id {
        Some(workflow_run_id) => require_run_workflow(db, ctx, workflow_run_id, needed).await,
        None => Err(not_found()),
    }
}

pub fn record_workflow_run_id(record: &Value) -> Option<Uuid> {
    record
        .get("workflow_run_id")
        .and_then(Value::as_str)
        .and_then(|raw| raw.parse::<Uuid>().ok())
}
