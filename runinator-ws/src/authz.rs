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
